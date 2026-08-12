#![forbid(unsafe_code)]

pub mod error;
pub mod plan;
pub mod recipe;
pub mod registry;
pub mod render;
pub mod resolver;
pub mod state;
pub mod transaction;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use error::{Result, ScafflareError};
use plan::{ChangeKind, FilePlan, PlannedChange};
use recipe::LoadedRecipe;
use registry::RecipeRegistry;
use render::{build_context, render_resolution};
use resolver::{Resolution, Resolver};
use state::{
    load_state, lockfile_path, serialize_state, InstalledRecipe, ManagedFile, ProjectState,
};
use transaction::{apply_plan, sha256_hex};

#[derive(Debug, Clone)]
pub struct GenerationRequest {
    pub project_name: String,
    pub recipes: Vec<String>,
    pub variables: BTreeMap<String, Value>,
    pub capabilities: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct GenerationPreview {
    pub resolution: Resolution,
    pub variables: BTreeMap<String, Value>,
    pub plan: FilePlan,
    pub state: ProjectState,
}

pub fn preview_init<R: RecipeRegistry>(
    root: &Path,
    registry: &R,
    request: GenerationRequest,
) -> Result<GenerationPreview> {
    if !is_valid_project_name(&request.project_name) {
        return Err(ScafflareError::InvalidInput(
            "project name must use lowercase letters, digits, hyphens or underscores".to_owned(),
        ));
    }
    if root.exists()
        && root
            .read_dir()
            .map_err(|error| ScafflareError::io(root, error))?
            .next()
            .is_some()
    {
        return Err(ScafflareError::FileConflict {
            path: root.to_path_buf(),
            reason:
                "init target must be empty; use `scafflare add` for an existing Scafflare project"
                    .to_owned(),
        });
    }
    let state = ProjectState::new(request.project_name, request.variables);
    preview_generation(root, registry, request.recipes, request.capabilities, state)
}

pub fn preview_add<R: RecipeRegistry>(
    root: &Path,
    registry: &R,
    recipes: Vec<String>,
    capabilities: BTreeSet<String>,
) -> Result<GenerationPreview> {
    let state = load_state(root)?;
    let mut combined: Vec<String> = state
        .recipes
        .iter()
        .map(|recipe| recipe.name.clone())
        .collect();
    for recipe in recipes {
        if !combined.contains(&recipe) {
            combined.push(recipe);
        }
    }
    preview_generation(root, registry, combined, capabilities, state)
}

fn preview_generation<R: RecipeRegistry>(
    root: &Path,
    registry: &R,
    recipes: Vec<String>,
    capabilities: BTreeSet<String>,
    state: ProjectState,
) -> Result<GenerationPreview> {
    let resolution = Resolver::new(registry, capabilities).resolve(&recipes)?;
    let variables = build_context(&resolution, &state.variables);
    let mut rendered = render_resolution(&resolution, &variables)?;
    for file in &mut rendered {
        if matches!(file.strategy, recipe::FileStrategy::Create)
            && state
                .managed_files
                .contains_key(&file.destination.to_string_lossy().to_string())
        {
            file.strategy = recipe::FileStrategy::Replace;
        }
    }
    let mut plan = plan::plan_files(root, &rendered)?;
    guard_modified_managed_files(root, &state, &mut plan, false)?;
    let mut next_state = state;
    next_state.variables = variables.clone();
    next_state.recipes = resolution
        .recipes
        .iter()
        .map(|recipe| InstalledRecipe {
            name: recipe.document.metadata.name.clone(),
            version: recipe.document.metadata.version.clone(),
        })
        .collect();
    update_managed_files(&mut next_state, &plan, &rendered);
    append_lockfile_change(root, &mut plan, &next_state)?;
    Ok(GenerationPreview {
        resolution,
        variables,
        plan,
        state: next_state,
    })
}

fn guard_modified_managed_files(
    root: &Path,
    state: &ProjectState,
    plan: &mut FilePlan,
    preserve: bool,
) -> Result<()> {
    for change in &mut plan.changes {
        if !matches!(
            change.kind,
            ChangeKind::Create | ChangeKind::Update | ChangeKind::Delete
        ) {
            continue;
        }
        let Some(managed) = state
            .managed_files
            .get(change.path.to_string_lossy().as_ref())
        else {
            continue;
        };
        let path = root.join(&change.path);
        if path.exists() {
            let current = fs::read(&path).map_err(|error| ScafflareError::io(&path, error))?;
            if sha256_hex(&current) != managed.sha256 {
                if preserve {
                    change.kind = ChangeKind::Skip;
                    change.contents = None;
                } else {
                    return Err(ScafflareError::FileConflict {
                        path: change.path.clone(),
                        reason: "refusing to overwrite a user-modified managed file".to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}

pub fn preview_remove<R: RecipeRegistry>(
    root: &Path,
    registry: &R,
    recipe: &str,
    capabilities: BTreeSet<String>,
) -> Result<(FilePlan, ProjectState)> {
    let current_state = load_state(root)?;
    if !current_state.has_recipe(recipe) {
        return Err(ScafflareError::InvalidInput(format!(
            "recipe `{recipe}` is not installed"
        )));
    }

    let removed = registry.get(recipe)?;
    let mut next_state = current_state.clone();
    next_state.remove_recipe(recipe);
    for key in removed.document.metadata.activation_variables.keys() {
        next_state.variables.remove(key);
    }

    let requested: Vec<String> = next_state
        .recipes
        .iter()
        .map(|installed| installed.name.clone())
        .collect();
    let resolution = Resolver::new(registry, capabilities).resolve(&requested)?;
    let variables = build_context(&resolution, &next_state.variables);
    next_state.variables = variables.clone();
    let mut rendered = render_resolution(&resolution, &variables)?;
    for file in &mut rendered {
        if current_state
            .managed_files
            .contains_key(&file.destination.to_string_lossy().to_string())
        {
            file.strategy = recipe::FileStrategy::Replace;
        }
    }
    let mut plan = plan::plan_files(root, &rendered)?;

    for (relative, managed) in &current_state.managed_files {
        let still_rendered = rendered
            .iter()
            .any(|file| file.destination == Path::new(relative));
        if !still_rendered && managed.recipes.len() == 1 && managed.recipes[0] == recipe {
            let path = root.join(relative);
            if path.exists() {
                plan.changes.push(PlannedChange {
                    kind: ChangeKind::Delete,
                    path: relative.into(),
                    recipe: recipe.to_owned(),
                    contents: None,
                });
            }
        }
    }

    guard_modified_managed_files(root, &current_state, &mut plan, true)?;

    next_state.recipes = resolution
        .recipes
        .iter()
        .map(|loaded| InstalledRecipe {
            name: loaded.document.metadata.name.clone(),
            version: loaded.document.metadata.version.clone(),
        })
        .collect();
    update_managed_files(&mut next_state, &plan, &rendered);
    append_lockfile_change(root, &mut plan, &next_state)?;
    Ok((plan, next_state))
}

pub fn commit(root: &Path, plan: &FilePlan) -> Result<()> {
    apply_plan(root, plan)
}

pub fn validation_commands(resolution: &Resolution) -> Vec<(String, String)> {
    resolution
        .recipes
        .iter()
        .flat_map(|recipe| {
            recipe
                .document
                .validation_commands
                .iter()
                .map(move |command| (recipe.document.metadata.name.clone(), command.display()))
        })
        .collect()
}

pub fn run_safe_validation_commands(
    resolution: &Resolution,
    root: &Path,
) -> Result<Vec<(String, bool)>> {
    let mut results = Vec::new();
    for recipe in &resolution.recipes {
        for command in &recipe.document.validation_commands {
            let Some((program, args)) = command.program_and_args() else {
                return Err(ScafflareError::InvalidRecipe {
                    recipe: recipe.document.metadata.name.clone(),
                    reason: "validation command must use argv YAML form to be executable"
                        .to_owned(),
                });
            };
            let status = std::process::Command::new(program)
                .args(args)
                .current_dir(root)
                .status()
                .map_err(|error| ScafflareError::Transaction {
                    path: root.to_path_buf(),
                    reason: format!("cannot start `{program}`: {error}"),
                })?;
            results.push((
                format!("{}: {}", recipe.document.metadata.name, command.display()),
                status.success(),
            ));
            if !status.success() {
                return Err(ScafflareError::Transaction {
                    path: root.to_path_buf(),
                    reason: format!("validation command `{}` failed", command.display()),
                });
            }
        }
    }
    Ok(results)
}

fn update_managed_files(
    state: &mut ProjectState,
    plan: &FilePlan,
    rendered: &[render::RenderedFile],
) {
    for change in &plan.changes {
        if matches!(change.kind, ChangeKind::Create | ChangeKind::Update)
            && change.path != Path::new(".scafflare/lock.yaml")
        {
            if let Some(contents) = &change.contents {
                let mut owners: Vec<String> = rendered
                    .iter()
                    .filter(|file| {
                        file.destination == change.path
                            && !matches!(file.strategy, recipe::FileStrategy::Skip)
                    })
                    .map(|file| file.recipe.clone())
                    .collect();
                owners.sort();
                owners.dedup();
                let file = state
                    .managed_files
                    .entry(change.path.to_string_lossy().to_string())
                    .or_insert_with(|| ManagedFile {
                        recipes: Vec::new(),
                        sha256: String::new(),
                    });
                file.recipes = owners;
                file.sha256 = sha256_hex(contents);
            }
        }
    }
}

fn append_lockfile_change(root: &Path, plan: &mut FilePlan, state: &ProjectState) -> Result<()> {
    let path = lockfile_path(root);
    let contents = serialize_state(state)?;
    let kind = match fs::read(&path) {
        Ok(existing) if existing == contents => ChangeKind::Unchanged,
        Ok(_) => ChangeKind::Update,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ChangeKind::Create,
        Err(error) => return Err(ScafflareError::io(&path, error)),
    };
    plan.changes.push(PlannedChange {
        kind,
        path: Path::new(".scafflare").join("lock.yaml"),
        recipe: "scafflare-state".to_owned(),
        contents: if kind == ChangeKind::Unchanged {
            None
        } else {
            Some(contents)
        },
    });
    plan.changes.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.recipe.cmp(&right.recipe))
    });
    Ok(())
}

fn is_valid_project_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
        && !value.starts_with('-')
        && !value.starts_with('_')
        && !value.ends_with('-')
        && !value.ends_with('_')
}

pub fn recipe_by_name<'a>(resolution: &'a Resolution, name: &str) -> Option<&'a LoadedRecipe> {
    resolution
        .recipes
        .iter()
        .find(|recipe| recipe.document.metadata.name == name)
}

#[cfg(test)]
mod integration_tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::PathBuf;

    use serde_json::Value;

    use super::*;
    use crate::plan::{ChangeKind, FilePlan, PlannedChange};
    use crate::registry::BundledRegistry;
    use crate::resolver::Resolver;

    fn minimal_request(name: &str) -> GenerationRequest {
        GenerationRequest {
            project_name: name.to_owned(),
            recipes: vec![
                "node".to_owned(),
                "typescript".to_owned(),
                "architecture-minimal".to_owned(),
            ],
            variables: BTreeMap::from([
                ("project_name".to_owned(), Value::String(name.to_owned())),
                ("project_slug".to_owned(), Value::String(name.to_owned())),
                (
                    "project_pascal".to_owned(),
                    Value::String("GoldenApi".to_owned()),
                ),
                ("framework".to_owned(), Value::String("none".to_owned())),
                (
                    "architecture".to_owned(),
                    Value::String("minimal".to_owned()),
                ),
                ("database".to_owned(), Value::String("none".to_owned())),
                ("zod_enabled".to_owned(), Value::Bool(false)),
                ("pino_enabled".to_owned(), Value::Bool(false)),
                ("vitest_enabled".to_owned(), Value::Bool(false)),
                ("biome_enabled".to_owned(), Value::Bool(false)),
            ]),
            capabilities: BTreeSet::from(["node_runtime".to_owned(), "npm".to_owned()]),
        }
    }

    #[test]
    fn generates_golden_minimal_project_and_is_idempotent() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("golden-api");
        let registry = BundledRegistry::new();
        let preview = preview_init(&root, &registry, minimal_request("golden-api")).unwrap();
        assert!(preview.plan.has_changes());
        commit(&root, &preview.plan).unwrap();

        let expected = include_str!("../../../fixtures/golden/minimal-package.json");
        let actual = fs::read_to_string(root.join("package.json")).unwrap();
        assert_eq!(actual, expected.replace("\r\n", "\n"));
        assert!(root.join(".scafflare/lock.yaml").is_file());

        let second = preview_add(
            &root,
            &registry,
            vec!["architecture-minimal".to_owned()],
            BTreeSet::from(["node_runtime".to_owned(), "npm".to_owned()]),
        )
        .unwrap();
        assert!(!second.plan.has_changes());
    }

    #[test]
    fn add_cross_cutting_recipe_rerenders_unchanged_managed_files_and_is_idempotent() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("safe-add");
        let registry = BundledRegistry::new();
        let initial = preview_init(&root, &registry, minimal_request("safe-add")).unwrap();
        commit(&root, &initial.plan).unwrap();

        let added = preview_add(
            &root,
            &registry,
            vec!["pino".to_owned()],
            BTreeSet::from(["node_runtime".to_owned(), "npm".to_owned()]),
        )
        .unwrap();
        assert!(added.plan.changes.iter().any(|change| {
            change.path == Path::new("package.json") && change.kind == ChangeKind::Update
        }));
        assert!(added.plan.changes.iter().any(|change| {
            change.path == Path::new("src/logger.ts") && change.kind == ChangeKind::Create
        }));
        commit(&root, &added.plan).unwrap();

        let repeated = preview_add(
            &root,
            &registry,
            vec!["pino".to_owned()],
            BTreeSet::from(["node_runtime".to_owned(), "npm".to_owned()]),
        )
        .unwrap();
        assert!(!repeated.plan.has_changes());
    }

    #[test]
    fn add_refuses_user_modified_managed_file() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("protected-add");
        let registry = BundledRegistry::new();
        let initial = preview_init(&root, &registry, minimal_request("protected-add")).unwrap();
        commit(&root, &initial.plan).unwrap();
        fs::write(root.join("package.json"), "{\"user\": true}\n").unwrap();

        let result = preview_add(
            &root,
            &registry,
            vec!["pino".to_owned()],
            BTreeSet::from(["node_runtime".to_owned(), "npm".to_owned()]),
        );
        assert!(matches!(result, Err(ScafflareError::FileConflict { .. })));
        assert_eq!(
            fs::read_to_string(root.join("package.json")).unwrap(),
            "{\"user\": true}\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn transaction_rejects_symlink_path_escape() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = temporary.path();
        symlink(outside.path(), root.join("src")).unwrap();
        let plan = FilePlan {
            changes: vec![PlannedChange {
                kind: ChangeKind::Create,
                path: PathBuf::from("src/escaped.ts"),
                recipe: "test".to_owned(),
                contents: Some(b"export {};\n".to_vec()),
            }],
        };

        let result = commit(root, &plan);
        assert!(matches!(result, Err(ScafflareError::Transaction { .. })));
        assert!(!outside.path().join("escaped.ts").exists());
    }

    #[test]
    fn resolver_rejects_official_framework_conflict() {
        let registry = BundledRegistry::new();
        let result = Resolver::new(&registry, BTreeSet::from(["node_runtime".to_owned()]))
            .resolve(&["express".to_owned(), "hono".to_owned()]);
        assert!(matches!(result, Err(ScafflareError::Conflict { .. })));
    }

    #[test]
    fn removal_preserves_user_modified_managed_file() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("safe-removal");
        let registry = BundledRegistry::new();
        let preview = preview_init(&root, &registry, minimal_request("safe-removal")).unwrap();
        commit(&root, &preview.plan).unwrap();
        fs::write(root.join("README.md"), "user modification\n").unwrap();
        let (plan, _) = preview_remove(
            &root,
            &registry,
            "architecture-minimal",
            BTreeSet::from(["node_runtime".to_owned(), "npm".to_owned()]),
        )
        .unwrap();
        assert!(
            plan.changes
                .iter()
                .any(|change| change.path == Path::new("README.md")
                    && change.kind == ChangeKind::Skip)
        );
        commit(&root, &plan).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("README.md")).unwrap(),
            "user modification\n"
        );
    }

    #[test]
    fn transaction_rolls_back_files_applied_before_later_failure() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        fs::create_dir(root.join("protected")).unwrap();
        let plan = FilePlan {
            changes: vec![
                PlannedChange {
                    kind: ChangeKind::Create,
                    path: PathBuf::from("created.txt"),
                    recipe: "test".to_owned(),
                    contents: Some(b"created\n".to_vec()),
                },
                PlannedChange {
                    kind: ChangeKind::Update,
                    path: PathBuf::from("protected"),
                    recipe: "test".to_owned(),
                    contents: Some(b"must-not-write\n".to_vec()),
                },
            ],
        };
        assert!(commit(root, &plan).is_err());
        assert!(!root.join("created.txt").exists());
        assert!(root.join("protected").is_dir());
    }
}
