#![forbid(unsafe_code)]

pub mod blobs;
pub mod error;
pub mod merge;
pub mod plan;
pub mod recipe;
pub mod reconcile;
pub mod registry;
pub mod render;
pub mod resolver;
pub mod state;
pub mod transaction;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use semver::Version;
use serde::Serialize;
use serde_json::Value;

use blobs::content_hash;
use error::{Result, ScafflareError};
use plan::FilePlan;
use recipe::LoadedRecipe;
use reconcile::{reconcile, unresolved_conflicts, Mode, ReconcileInput};
pub use reconcile::{Conflict, ConflictKind, ConflictMode, Outcome, OutcomeKind};
use registry::RecipeRegistry;
use render::{build_context, render_resolution};
use resolver::{Resolution, Resolver};
use state::{load_state, InstalledRecipe, ProjectState};
use transaction::apply_plan;

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
    pub outcomes: Vec<Outcome>,
    pub conflicts: Vec<Conflict>,
}

impl GenerationPreview {
    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct UpgradeOptions {
    pub conflict_mode: ConflictMode,
    /// Variable overrides (`--set`) applied on top of the stored variables.
    pub variables: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct UpgradePreview {
    pub preview: GenerationPreview,
    pub from: Vec<InstalledRecipe>,
    pub to: Vec<InstalledRecipe>,
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
    preview_generation(
        root,
        registry,
        request.recipes,
        request.capabilities,
        state,
        Mode::Init,
        ConflictMode::Abort,
    )
}

pub fn preview_add<R: RecipeRegistry>(
    root: &Path,
    registry: &R,
    recipes: Vec<String>,
    capabilities: BTreeSet<String>,
) -> Result<GenerationPreview> {
    preview_add_with(root, registry, recipes, capabilities, ConflictMode::Abort)
}

pub fn preview_add_with<R: RecipeRegistry>(
    root: &Path,
    registry: &R,
    recipes: Vec<String>,
    capabilities: BTreeSet<String>,
    conflict_mode: ConflictMode,
) -> Result<GenerationPreview> {
    let state = load_state(root)?;
    ensure_conflicts_resolved(root, &state)?;
    ensure_up_to_date(registry, &state)?;
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
    preview_generation(
        root,
        registry,
        combined,
        capabilities,
        state,
        Mode::Add,
        conflict_mode,
    )
}

/// Bring every installed recipe to the registry version with a three-way merge.
pub fn preview_upgrade<R: RecipeRegistry>(
    root: &Path,
    registry: &R,
    capabilities: BTreeSet<String>,
    options: UpgradeOptions,
) -> Result<UpgradePreview> {
    let mut state = load_state(root)?;
    ensure_conflicts_resolved(root, &state)?;
    for installed in &state.recipes {
        let available = registry.get(&installed.name)?.document.metadata.version;
        if available < installed.version {
            return Err(ScafflareError::Downgrade {
                recipe: installed.name.clone(),
                installed: installed.version.to_string(),
                available: available.to_string(),
            });
        }
    }
    let from = state.recipes.clone();
    let requested: Vec<String> = from.iter().map(|recipe| recipe.name.clone()).collect();
    state.variables.extend(options.variables);
    let preview = preview_generation(
        root,
        registry,
        requested,
        capabilities,
        state,
        Mode::Upgrade,
        options.conflict_mode,
    )?;
    let to = preview.state.recipes.clone();
    Ok(UpgradePreview { preview, from, to })
}

fn ensure_up_to_date<R: RecipeRegistry>(registry: &R, state: &ProjectState) -> Result<()> {
    for installed in &state.recipes {
        let available = registry.get(&installed.name)?.document.metadata.version;
        if available != installed.version {
            return Err(ScafflareError::OutdatedProject {
                recipe: installed.name.clone(),
                installed: installed.version.to_string(),
                available: available.to_string(),
            });
        }
    }
    Ok(())
}

fn ensure_conflicts_resolved(root: &Path, state: &ProjectState) -> Result<()> {
    let unresolved = unresolved_conflicts(root, &state.managed_files);
    if unresolved.is_empty() {
        Ok(())
    } else {
        Err(ScafflareError::UnresolvedConflicts {
            paths: unresolved.join(", "),
        })
    }
}

fn installed_recipes(resolution: &Resolution) -> Vec<InstalledRecipe> {
    resolution
        .recipes
        .iter()
        .map(|recipe| InstalledRecipe {
            name: recipe.document.metadata.name.clone(),
            version: recipe.document.metadata.version.clone(),
        })
        .collect()
}

fn preview_generation<R: RecipeRegistry>(
    root: &Path,
    registry: &R,
    recipes: Vec<String>,
    capabilities: BTreeSet<String>,
    state: ProjectState,
    mode: Mode,
    conflict_mode: ConflictMode,
) -> Result<GenerationPreview> {
    let resolution = Resolver::new(registry, capabilities).resolve(&recipes)?;
    let variables = build_context(&resolution, &state.variables);
    let rendered = render_resolution(&resolution, &variables)?;
    let mut next_state = state.clone();
    next_state.variables = variables.clone();
    next_state.recipes = installed_recipes(&resolution);
    let reconciled = reconcile(ReconcileInput {
        root,
        rendered: &rendered,
        old_managed: &state.managed_files,
        next_state,
        mode,
        conflict_mode,
    })?;
    Ok(GenerationPreview {
        resolution,
        variables,
        plan: reconciled.plan,
        state: reconciled.state,
        outcomes: reconciled.outcomes,
        conflicts: reconciled.conflicts,
    })
}

pub fn preview_remove<R: RecipeRegistry>(
    root: &Path,
    registry: &R,
    recipe: &str,
    capabilities: BTreeSet<String>,
) -> Result<(FilePlan, ProjectState)> {
    let current_state = load_state(root)?;
    ensure_up_to_date(registry, &current_state)?;
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
    next_state.recipes = installed_recipes(&resolution);
    let rendered = render_resolution(&resolution, &variables)?;
    let reconciled = reconcile(ReconcileInput {
        root,
        rendered: &rendered,
        old_managed: &current_state.managed_files,
        next_state,
        mode: Mode::Remove,
        conflict_mode: ConflictMode::Abort,
    })?;
    Ok((reconciled.plan, reconciled.state))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeState {
    Current,
    Outdated,
    NewerThanRegistry,
    MissingFromRegistry,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecipeStatus {
    pub name: String,
    pub installed: Version,
    pub available: Option<Version>,
    pub state: RecipeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Clean,
    Modified,
    Missing,
    Conflicted,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileStatus {
    pub path: String,
    pub state: FileState,
    /// False for files recorded by a version 1 lockfile (no merge ancestor).
    pub has_base: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectStatus {
    pub project_name: String,
    pub recipes: Vec<RecipeStatus>,
    pub files: Vec<FileStatus>,
    pub outdated: bool,
    pub conflicted: bool,
    pub modified: usize,
    pub missing: usize,
    pub legacy_files: usize,
}

impl ProjectStatus {
    /// True when anything deserves attention (used by `status --exit-code`).
    pub fn needs_attention(&self) -> bool {
        self.outdated || self.conflicted || self.modified > 0 || self.missing > 0
    }
}

pub fn project_status<R: RecipeRegistry>(root: &Path, registry: &R) -> Result<ProjectStatus> {
    let state = load_state(root)?;
    let mut recipes = Vec::new();
    let mut outdated = false;
    for installed in &state.recipes {
        let available = registry
            .get(&installed.name)
            .ok()
            .map(|recipe| recipe.document.metadata.version);
        let recipe_state = match &available {
            None => RecipeState::MissingFromRegistry,
            Some(version) if *version == installed.version => RecipeState::Current,
            Some(version) if *version > installed.version => RecipeState::Outdated,
            Some(_) => RecipeState::NewerThanRegistry,
        };
        outdated |= recipe_state == RecipeState::Outdated;
        recipes.push(RecipeStatus {
            name: installed.name.clone(),
            installed: installed.version.clone(),
            available,
            state: recipe_state,
        });
    }

    let unresolved: BTreeSet<String> = unresolved_conflicts(root, &state.managed_files)
        .into_iter()
        .collect();
    let mut files = Vec::new();
    let (mut modified, mut missing, mut legacy_files) = (0, 0, 0);
    for (path, managed) in &state.managed_files {
        let file_state = if unresolved.contains(path) {
            FileState::Conflicted
        } else {
            match fs::read(root.join(path)) {
                Err(_) => FileState::Missing,
                Ok(bytes) if content_hash(&bytes) == managed.sha256 => FileState::Clean,
                Ok(_) => FileState::Modified,
            }
        };
        match file_state {
            FileState::Modified => modified += 1,
            FileState::Missing => missing += 1,
            _ => {}
        }
        if managed.base.is_none() {
            legacy_files += 1;
        }
        files.push(FileStatus {
            path: path.clone(),
            state: file_state,
            has_base: managed.base.is_some(),
        });
    }
    Ok(ProjectStatus {
        project_name: state.project_name,
        recipes,
        files,
        outdated,
        conflicted: !unresolved.is_empty(),
        modified,
        missing,
        legacy_files,
    })
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
    fn add_never_overwrites_a_conflicting_user_modified_managed_file() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("protected-add");
        let registry = BundledRegistry::new();
        let initial = preview_init(&root, &registry, minimal_request("protected-add")).unwrap();
        commit(&root, &initial.plan).unwrap();
        fs::write(root.join("package.json"), "{\"user\": true}\n").unwrap();

        let preview = preview_add(
            &root,
            &registry,
            vec!["pino".to_owned()],
            BTreeSet::from(["node_runtime".to_owned(), "npm".to_owned()]),
        )
        .unwrap();
        assert!(preview.has_conflicts());
        assert!(
            !preview.plan.has_changes(),
            "abort mode must plan no writes"
        );
        commit(&root, &preview.plan).unwrap();
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
