use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};

use crate::error::{Result, ScafflareError};
use crate::recipe::{LoadedRecipe, RecipeDocument};

static OFFICIAL_RECIPES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../recipes/official");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrySource {
    Bundled,
    Filesystem,
}

pub trait RecipeRegistry {
    fn list(&self) -> Result<Vec<String>>;
    fn get(&self, name: &str) -> Result<LoadedRecipe>;
    fn source(&self) -> RegistrySource;
}

#[derive(Debug, Default, Clone)]
pub struct BundledRegistry;

impl BundledRegistry {
    pub fn new() -> Self {
        Self
    }
}

impl RecipeRegistry for BundledRegistry {
    fn list(&self) -> Result<Vec<String>> {
        let mut names: Vec<String> = OFFICIAL_RECIPES
            .dirs()
            .filter_map(|dir| dir.path().file_name())
            .filter_map(|name| name.to_str())
            .map(ToOwned::to_owned)
            .collect();
        names.sort();
        Ok(names)
    }

    fn get(&self, name: &str) -> Result<LoadedRecipe> {
        let directory = OFFICIAL_RECIPES
            .get_dir(name)
            .ok_or_else(|| ScafflareError::RecipeNotFound(name.to_owned()))?;
        load_embedded_recipe(directory, name)
    }

    fn source(&self) -> RegistrySource {
        RegistrySource::Bundled
    }
}

#[derive(Debug, Clone)]
pub struct FilesystemRegistry {
    root: PathBuf,
}

impl FilesystemRegistry {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl RecipeRegistry for FilesystemRegistry {
    fn list(&self) -> Result<Vec<String>> {
        let entries =
            fs::read_dir(&self.root).map_err(|error| ScafflareError::io(&self.root, error))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| ScafflareError::io(&self.root, error))?;
            if entry.path().join("recipe.yaml").is_file() {
                names.push(entry.file_name().to_string_lossy().to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    fn get(&self, name: &str) -> Result<LoadedRecipe> {
        let root = self.root.join(name);
        let recipe_path = root.join("recipe.yaml");
        if !recipe_path.is_file() {
            return Err(ScafflareError::RecipeNotFound(name.to_owned()));
        }
        let raw = fs::read_to_string(&recipe_path)
            .map_err(|error| ScafflareError::io(&recipe_path, error))?;
        let document = RecipeDocument::from_yaml(name, &raw)?;
        let mut templates = BTreeMap::new();
        let templates_root = root.join("templates");
        if templates_root.exists() {
            for entry in walkdir::WalkDir::new(&templates_root) {
                let entry = entry.map_err(|error| ScafflareError::InvalidRecipe {
                    recipe: name.to_owned(),
                    reason: error.to_string(),
                })?;
                if entry.file_type().is_file() {
                    let relative = entry.path().strip_prefix(&root).map_err(|error| {
                        ScafflareError::InvalidRecipe {
                            recipe: name.to_owned(),
                            reason: error.to_string(),
                        }
                    })?;
                    let relative = relative.to_string_lossy().replace('\\', "/");
                    let content = fs::read(entry.path())
                        .map_err(|error| ScafflareError::io(entry.path(), error))?;
                    templates.insert(relative, content);
                }
            }
        }
        Ok(LoadedRecipe {
            document,
            templates,
        })
    }

    fn source(&self) -> RegistrySource {
        RegistrySource::Filesystem
    }
}

#[derive(Debug, Clone)]
pub struct CompositeRegistry {
    local: Vec<FilesystemRegistry>,
    bundled: BundledRegistry,
}

impl CompositeRegistry {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            local: roots.into_iter().map(FilesystemRegistry::new).collect(),
            bundled: BundledRegistry::new(),
        }
    }
}

impl RecipeRegistry for CompositeRegistry {
    fn list(&self) -> Result<Vec<String>> {
        let mut names = self.bundled.list()?;
        for registry in &self.local {
            names.extend(registry.list()?);
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn get(&self, name: &str) -> Result<LoadedRecipe> {
        for registry in &self.local {
            if let Ok(recipe) = registry.get(name) {
                return Ok(recipe);
            }
        }
        self.bundled.get(name)
    }

    fn source(&self) -> RegistrySource {
        if self.local.is_empty() {
            RegistrySource::Bundled
        } else {
            RegistrySource::Filesystem
        }
    }
}

fn load_embedded_recipe(directory: &Dir<'_>, recipe_name: &str) -> Result<LoadedRecipe> {
    let recipe_file = directory
        .files()
        .find(|file| file.path().file_name().and_then(|name| name.to_str()) == Some("recipe.yaml"))
        .ok_or_else(|| ScafflareError::InvalidRecipe {
            recipe: recipe_name.to_owned(),
            reason: "missing recipe.yaml".to_owned(),
        })?;
    let raw = recipe_file
        .contents_utf8()
        .ok_or_else(|| ScafflareError::InvalidRecipe {
            recipe: recipe_name.to_owned(),
            reason: "recipe.yaml is not UTF-8".to_owned(),
        })?;
    let document = RecipeDocument::from_yaml(recipe_name, raw)?;
    let mut templates = BTreeMap::new();
    if let Some(template_dir) = directory
        .dirs()
        .find(|dir| dir.path().file_name().and_then(|name| name.to_str()) == Some("templates"))
    {
        collect_embedded_files(template_dir, "templates", &mut templates);
    }
    Ok(LoadedRecipe {
        document,
        templates,
    })
}

fn collect_embedded_files(
    directory: &Dir<'_>,
    relative_prefix: &str,
    output: &mut BTreeMap<String, Vec<u8>>,
) {
    for file in directory.files() {
        let Some(name) = file.path().file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        output.insert(
            format!("{relative_prefix}/{name}"),
            file.contents().to_vec(),
        );
    }
    for child in directory.dirs() {
        let Some(name) = child.path().file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        collect_embedded_files(child, &format!("{relative_prefix}/{name}"), output);
    }
}

pub fn recipe_root_from_path(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| parent.join("recipe.yaml").is_file())
}
