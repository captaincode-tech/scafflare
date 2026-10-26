//! Integration scenarios for three-way upgrade, add, and remove.
//!
//! A versioned filesystem registry is rebuilt between "v1" and "v2" of a small
//! test recipe so every reconcile rule is exercised against real files.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use scafflare_core::blobs::{content_hash, list_blobs, read_blob};
use scafflare_core::error::ScafflareError;
use scafflare_core::plan::ChangeKind;
use scafflare_core::registry::FilesystemRegistry;
use scafflare_core::state::{load_state, serialize_state};
use scafflare_core::transaction::{apply_plan_limited, ordered_changes};
use scafflare_core::{
    commit, preview_add, preview_add_with, preview_init, preview_remove, preview_upgrade,
    project_status, ConflictKind, ConflictMode, FileState, GenerationRequest, OutcomeKind,
    UpgradeOptions, UpgradePreview,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Recipe fixtures
// ---------------------------------------------------------------------------

struct File {
    dest: &'static str,
    strategy: &'static str,
    body: String,
    when: Option<&'static str>,
}

fn file(dest: &'static str, strategy: &'static str, body: impl Into<String>) -> File {
    File {
        dest,
        strategy,
        body: body.into(),
        when: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn write_recipe(
    registry: &Path,
    name: &str,
    version: &str,
    deps: &[&str],
    conflicts: &[&str],
    variables: &[(&str, &str)],
    files: &[File],
) {
    let dir = registry.join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("templates")).unwrap();
    let mut yaml = format!(
        "schema_version: 1\nmetadata:\n  name: {name}\n  version: {version}\n  description: test recipe\n"
    );
    if !variables.is_empty() {
        yaml.push_str("variables:\n");
        for (key, value) in variables {
            yaml.push_str(&format!("  {key}: {value}\n"));
        }
    }
    yaml.push_str(&format!("depends_on: [{}]\n", deps.join(", ")));
    yaml.push_str(&format!("conflicts: [{}]\n", conflicts.join(", ")));
    yaml.push_str("files:\n");
    for (index, file) in files.iter().enumerate() {
        // MiniJinja drops one trailing newline, so add one to keep rendered files newline-terminated.
        fs::write(
            dir.join(format!("templates/f{index}.jinja")),
            format!("{}\n", file.body),
        )
        .unwrap();
        yaml.push_str(&format!(
            "  - source: templates/f{index}.jinja\n    destination: {}\n    strategy: {}\n",
            file.dest, file.strategy
        ));
        if let Some(when) = file.when {
            yaml.push_str(&format!("    when: \"{when}\"\n"));
        }
    }
    fs::write(dir.join("recipe.yaml"), yaml).unwrap();
}

fn main_ts(edits: &[(usize, &str)]) -> String {
    (1..=12)
        .map(|n| {
            edits
                .iter()
                .find(|(line, _)| *line == n)
                .map(|(_, text)| (*text).to_owned())
                .unwrap_or_else(|| format!("line{n:02}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn package_json(version: u32) -> String {
    if version == 1 {
        r#"{"name":"demo","scripts":{"build":"tsc"},"dependencies":{"zod":"^3.0.0"}}"#.to_owned()
    } else {
        r#"{"name":"demo","scripts":{"build":"tsc","lint":"biome"},"dependencies":{"zod":"^3.0.0","pino":"^9.0.0"}}"#.to_owned()
    }
}

fn write_app(registry: &Path, version: u32) {
    let mut files = vec![
        file("package.json", "merge_json", package_json(version)),
        file("stable.txt", "create", "s1\ns2\ns3\ns4\ns5\ns6\n"),
    ];
    if version == 1 {
        files.push(file("README.md", "create", "# Demo\n\nHello\n"));
        files.push(file("src/main.ts", "create", main_ts(&[])));
        files.push(file("legacy.txt", "create", "legacy\n"));
        write_recipe(registry, "app", "0.1.0", &[], &[], &[], &files);
    } else {
        files.push(file("README.md", "create", "# Demo v2\n\nHello\n"));
        files.push(file("src/main.ts", "create", main_ts(&[(10, "line10 v2")])));
        files.push(file("added.txt", "create", "added in v2\n"));
        files.push(file("greeting.txt", "create", "{{ greeting }}\n"));
        write_recipe(
            registry,
            "app",
            "0.2.0",
            &[],
            &[],
            &[("greeting", "hello")],
            &files,
        );
    }
}

fn write_extra(registry: &Path) {
    write_recipe(
        registry,
        "extra",
        "0.1.0",
        &[],
        &[],
        &[],
        &[
            file("extra.txt", "create", "extra\n"),
            file(
                "package.json",
                "merge_json",
                r#"{"scripts":{"extra":"node extra"}}"#,
            ),
        ],
    );
}

fn write_other(registry: &Path) {
    write_recipe(
        registry,
        "other",
        "0.1.0",
        &[],
        &[],
        &[],
        &[file("other.txt", "create", "other\n")],
    );
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    registry_dir: PathBuf,
}

fn caps() -> BTreeSet<String> {
    BTreeSet::new()
}

impl Fixture {
    /// Initialized project at app v1 (plus the given extra recipes).
    fn new(extra: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let registry_dir = dir.path().join("registry");
        fs::create_dir_all(&registry_dir).unwrap();
        write_app(&registry_dir, 1);
        write_extra(&registry_dir);
        write_other(&registry_dir);
        let root = dir.path().join("proj");
        let fixture = Self {
            _dir: dir,
            root,
            registry_dir,
        };
        let mut recipes = vec!["app".to_owned()];
        recipes.extend(extra.iter().map(|name| (*name).to_owned()));
        let preview = preview_init(
            &fixture.root,
            &fixture.registry(),
            GenerationRequest {
                project_name: "demo".to_owned(),
                recipes,
                variables: BTreeMap::new(),
                capabilities: caps(),
            },
        )
        .unwrap();
        commit(&fixture.root, &preview.plan).unwrap();
        assert_state_invariants(&fixture.root);
        fixture
    }

    fn registry(&self) -> FilesystemRegistry {
        FilesystemRegistry::new(&self.registry_dir)
    }

    fn to_v2(&self) {
        write_app(&self.registry_dir, 2);
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).unwrap()
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn try_upgrade(&self, mode: ConflictMode) -> Result<UpgradePreview, ScafflareError> {
        preview_upgrade(
            &self.root,
            &self.registry(),
            caps(),
            UpgradeOptions {
                conflict_mode: mode,
                variables: BTreeMap::new(),
            },
        )
    }

    fn upgrade(&self, mode: ConflictMode) -> UpgradePreview {
        self.try_upgrade(mode).unwrap()
    }

    fn upgrade_apply(&self, mode: ConflictMode) -> UpgradePreview {
        let preview = self.upgrade(mode);
        commit(&self.root, &preview.preview.plan).unwrap();
        preview
    }

    fn outcome(&self, preview: &UpgradePreview, path: &str) -> OutcomeKind {
        preview
            .preview
            .outcomes
            .iter()
            .find(|outcome| outcome.path == Path::new(path))
            .unwrap_or_else(|| panic!("no outcome for {path}"))
            .kind
    }

    fn tree_hash(&self) -> String {
        tree_hash(&self.root)
    }
}

fn tree_hash(root: &Path) -> String {
    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries.sort();
    let mut hasher = Sha256::new();
    for (path, bytes) in entries {
        hasher.update(path.as_bytes());
        hasher.update(&bytes);
    }
    format!("{:x}", hasher.finalize())
}

fn collect(root: &Path, directory: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let relative = path.strip_prefix(root).unwrap();
            out.push((
                relative.to_string_lossy().replace('\\', "/"),
                fs::read(&path).unwrap(),
            ));
        }
    }
}

/// Invariants that must hold after every successful Scafflare commit.
fn assert_state_invariants(root: &Path) {
    check_invariants(root, false);
}

fn check_invariants(root: &Path, allow_legacy: bool) {
    let state = load_state(root).unwrap();
    let mut referenced = BTreeSet::new();
    for (path, managed) in &state.managed_files {
        match &managed.base {
            Some(base) => {
                assert!(read_blob(root, base).is_some(), "blob for {path} invalid");
                referenced.insert(base.clone());
            }
            None => assert!(allow_legacy, "managed file {path} has no base"),
        }
        if let Ok(bytes) = fs::read(root.join(path)) {
            assert_eq!(content_hash(&bytes), managed.sha256, "sha256 drift: {path}");
        }
    }
    assert_eq!(list_blobs(root).unwrap(), referenced, "orphan blobs");
    assert!(
        !root.join(".scafflare/transactions").exists(),
        "transaction leftovers"
    );
}

fn json_file(fixture: &Fixture, path: &str) -> Value {
    serde_json::from_str(&fixture.read(path)).unwrap()
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

#[test]
fn s01_unchanged_project_is_a_noop() {
    let fixture = Fixture::new(&[]);
    let before = fixture.tree_hash();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert!(!preview.preview.plan.has_changes());
    assert!(!preview.preview.has_conflicts());
    assert_eq!(fixture.tree_hash(), before);
    assert_eq!(preview.from, preview.to);
}

#[test]
fn s02_template_change_fast_forwards_untouched_files() {
    let fixture = Fixture::new(&[]);
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(fixture.outcome(&preview, "README.md"), OutcomeKind::Updated);
    assert_eq!(fixture.read("README.md"), "# Demo v2\n\nHello\n");
    assert_eq!(fixture.read("src/main.ts"), main_ts(&[(10, "line10 v2")]));
    assert_eq!(fixture.read("added.txt"), "added in v2\n");
    assert_eq!(fixture.read("greeting.txt"), "hello\n");
    assert!(!fixture.path("legacy.txt").exists());
    assert_eq!(
        load_state(&fixture.root).unwrap().recipes[0]
            .version
            .to_string(),
        "0.2.0"
    );
    assert_state_invariants(&fixture.root);
}

#[test]
fn s03_user_change_is_kept_when_template_did_not_change() {
    let fixture = Fixture::new(&[]);
    fixture.write("stable.txt", "s1\nUSER\ns3\ns4\ns5\ns6\n");
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(fixture.outcome(&preview, "stable.txt"), OutcomeKind::Kept);
    assert_eq!(fixture.read("stable.txt"), "s1\nUSER\ns3\ns4\ns5\ns6\n");
    assert_state_invariants(&fixture.root);
}

#[test]
fn s04_edits_on_different_lines_merge_cleanly() {
    let fixture = Fixture::new(&[]);
    fixture.write("src/main.ts", &main_ts(&[(2, "USER line two")]));
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(
        fixture.outcome(&preview, "src/main.ts"),
        OutcomeKind::Merged
    );
    assert_eq!(
        fixture.read("src/main.ts"),
        main_ts(&[(2, "USER line two"), (10, "line10 v2")])
    );
    assert_state_invariants(&fixture.root);
}

#[test]
fn s05_conflict_in_abort_mode_leaves_tree_byte_identical() {
    let fixture = Fixture::new(&[]);
    fixture.write("src/main.ts", &main_ts(&[(10, "USER line ten")]));
    fixture.to_v2();
    let before = fixture.tree_hash();
    let preview = fixture.upgrade(ConflictMode::Abort);
    assert!(preview.preview.has_conflicts());
    assert_eq!(
        preview.preview.conflicts[0].kind,
        ConflictKind::BothModified
    );
    assert_eq!(preview.preview.conflicts[0].path, Path::new("src/main.ts"));
    assert!(!preview.preview.plan.has_changes());
    commit(&fixture.root, &preview.preview.plan).unwrap();
    assert_eq!(fixture.tree_hash(), before);
}

#[test]
fn s06_markers_mode_writes_markers_and_blocks_until_resolved() {
    let fixture = Fixture::new(&[]);
    fixture.write("src/main.ts", &main_ts(&[(10, "USER line ten")]));
    fixture.to_v2();
    fixture.upgrade_apply(ConflictMode::Markers);
    let marked = fixture.read("src/main.ts");
    assert!(marked.contains("<<<<<<<") && marked.contains(">>>>>>>"));
    assert!(marked.contains("USER line ten") && marked.contains("line10 v2"));
    assert_state_invariants(&fixture.root);

    let status = project_status(&fixture.root, &fixture.registry()).unwrap();
    assert!(status.conflicted && status.needs_attention());
    assert!(status
        .files
        .iter()
        .any(|f| f.path == "src/main.ts" && f.state == FileState::Conflicted));

    assert!(matches!(
        fixture.try_upgrade(ConflictMode::Abort),
        Err(ScafflareError::UnresolvedConflicts { .. })
    ));
    assert!(matches!(
        preview_add(
            &fixture.root,
            &fixture.registry(),
            vec!["extra".into()],
            caps()
        ),
        Err(ScafflareError::UnresolvedConflicts { .. })
    ));

    let resolved = main_ts(&[(10, "RESOLVED line ten")]);
    fixture.write("src/main.ts", &resolved);
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert!(!preview.preview.has_conflicts());
    assert_eq!(fixture.read("src/main.ts"), resolved);
    assert_state_invariants(&fixture.root);
    assert!(
        !project_status(&fixture.root, &fixture.registry())
            .unwrap()
            .conflicted
    );
}

#[test]
fn s07_package_json_keeps_user_dependency_and_gains_new_script() {
    let fixture = Fixture::new(&[]);
    let mut manifest = json_file(&fixture, "package.json");
    manifest["dependencies"]["dayjs"] = json!("^1.11.0");
    manifest["scripts"]["dev"] = json!("tsx watch");
    fixture.write(
        "package.json",
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(
        fixture.outcome(&preview, "package.json"),
        OutcomeKind::Merged
    );
    let merged = json_file(&fixture, "package.json");
    assert_eq!(merged["dependencies"]["dayjs"], "^1.11.0");
    assert_eq!(merged["dependencies"]["pino"], "^9.0.0");
    assert_eq!(merged["scripts"]["dev"], "tsx watch");
    assert_eq!(merged["scripts"]["lint"], "biome");
    assert_state_invariants(&fixture.root);
}

#[test]
fn s08_package_json_conflict_reports_pointer_and_writes_sidecar_in_markers_mode() {
    let fixture = Fixture::new(&[]);
    let mut manifest = json_file(&fixture, "package.json");
    manifest["scripts"]["build"] = json!("tsc --noEmit false");
    fixture.write(
        "package.json",
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );
    // v2 changes the same script.
    write_recipe(
        &fixture.registry_dir,
        "app",
        "0.2.0",
        &[],
        &[],
        &[],
        &[
            file(
                "package.json",
                "merge_json",
                r#"{"name":"demo","scripts":{"build":"tsup"},"dependencies":{"zod":"^3.0.0"}}"#,
            ),
            file("stable.txt", "create", "s1\ns2\ns3\ns4\ns5\ns6\n"),
            file("README.md", "create", "# Demo\n\nHello\n"),
            file("src/main.ts", "create", main_ts(&[])),
            file("legacy.txt", "create", "legacy\n"),
        ],
    );
    let before = fixture.read("package.json");
    let preview = fixture.upgrade(ConflictMode::Abort);
    assert_eq!(preview.preview.conflicts.len(), 1);
    assert_eq!(
        preview.preview.conflicts[0].json_paths,
        vec!["/scripts/build"]
    );

    fixture.upgrade_apply(ConflictMode::Markers);
    assert_eq!(fixture.read("package.json"), before, "user file untouched");
    let sidecar = json_file(&fixture, "package.json.scafflare-new");
    assert_eq!(sidecar["scripts"]["build"], "tsup");
    assert!(
        project_status(&fixture.root, &fixture.registry())
            .unwrap()
            .conflicted
    );
    assert!(matches!(
        fixture.try_upgrade(ConflictMode::Abort),
        Err(ScafflareError::UnresolvedConflicts { .. })
    ));
    fs::remove_file(fixture.path("package.json.scafflare-new")).unwrap();
    let after = fixture.upgrade_apply(ConflictMode::Abort);
    assert!(!after.preview.has_conflicts());
    assert_eq!(fixture.read("package.json"), before);
}

#[test]
fn s09_new_unmanaged_file_is_adopted_when_identical_and_conflicts_otherwise() {
    let fixture = Fixture::new(&[]);
    fixture.write("added.txt", "added in v2\n");
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(fixture.outcome(&preview, "added.txt"), OutcomeKind::Adopted);
    assert!(load_state(&fixture.root)
        .unwrap()
        .managed_files
        .contains_key("added.txt"));

    let fixture = Fixture::new(&[]);
    fixture.write("added.txt", "my own file\n");
    fixture.to_v2();
    let preview = fixture.upgrade(ConflictMode::Abort);
    assert_eq!(preview.preview.conflicts.len(), 1);
    assert_eq!(
        preview.preview.conflicts[0].kind,
        ConflictKind::UnmanagedExists
    );
    fixture.upgrade_apply(ConflictMode::Markers);
    assert_eq!(fixture.read("added.txt"), "my own file\n");
    assert_eq!(fixture.read("added.txt.scafflare-new"), "added in v2\n");
}

#[test]
fn s10_removed_template_file_is_deleted_or_orphaned() {
    let fixture = Fixture::new(&[]);
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(
        fixture.outcome(&preview, "legacy.txt"),
        OutcomeKind::Deleted
    );
    assert!(!fixture.path("legacy.txt").exists());

    let fixture = Fixture::new(&[]);
    fixture.write("legacy.txt", "legacy plus my notes\n");
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(
        fixture.outcome(&preview, "legacy.txt"),
        OutcomeKind::Orphaned
    );
    assert_eq!(fixture.read("legacy.txt"), "legacy plus my notes\n");
    assert!(!load_state(&fixture.root)
        .unwrap()
        .managed_files
        .contains_key("legacy.txt"));
    assert_state_invariants(&fixture.root);
}

#[test]
fn s11_user_deleted_files_honor_or_conflict_depending_on_template() {
    // Template unchanged for stable.txt: deletion is honored and sticks.
    let fixture = Fixture::new(&[]);
    fs::remove_file(fixture.path("stable.txt")).unwrap();
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(
        fixture.outcome(&preview, "stable.txt"),
        OutcomeKind::DeletionHonored
    );
    assert!(!fixture.path("stable.txt").exists());
    fixture.upgrade_apply(ConflictMode::Abort);
    assert!(
        !fixture.path("stable.txt").exists(),
        "must not be resurrected"
    );
    assert_state_invariants(&fixture.root);

    // Template changed for README.md: deleted-by-user conflict.
    let fixture = Fixture::new(&[]);
    fs::remove_file(fixture.path("README.md")).unwrap();
    fixture.to_v2();
    let preview = fixture.upgrade(ConflictMode::Abort);
    assert_eq!(
        preview.preview.conflicts[0].kind,
        ConflictKind::DeletedByUser
    );
    assert!(!fixture.path("README.md").exists());
}

#[test]
fn s12_new_dependency_in_v2_is_installed() {
    let fixture = Fixture::new(&[]);
    write_recipe(
        &fixture.registry_dir,
        "app",
        "0.2.0",
        &["extra"],
        &[],
        &[],
        &[
            file("package.json", "merge_json", package_json(1)),
            file("stable.txt", "create", "s1\ns2\ns3\ns4\ns5\ns6\n"),
            file("README.md", "create", "# Demo\n\nHello\n"),
            file("src/main.ts", "create", main_ts(&[])),
            file("legacy.txt", "create", "legacy\n"),
        ],
    );
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert!(preview.to.iter().any(|recipe| recipe.name == "extra"));
    assert_eq!(fixture.read("extra.txt"), "extra\n");
    assert_eq!(
        json_file(&fixture, "package.json")["scripts"]["extra"],
        "node extra"
    );
    assert_state_invariants(&fixture.root);
}

#[test]
fn s13_resolver_conflict_in_v2_fails_before_any_write() {
    let fixture = Fixture::new(&["other"]);
    write_recipe(
        &fixture.registry_dir,
        "app",
        "0.2.0",
        &[],
        &["other"],
        &[],
        &[file("package.json", "merge_json", package_json(1))],
    );
    let before = fixture.tree_hash();
    assert!(matches!(
        fixture.try_upgrade(ConflictMode::Abort),
        Err(ScafflareError::Conflict { .. })
    ));
    assert_eq!(fixture.tree_hash(), before);
}

#[test]
fn s14_new_variables_use_defaults_and_set_overrides() {
    let fixture = Fixture::new(&[]);
    fixture.to_v2();
    let overridden = preview_upgrade(
        &fixture.root,
        &fixture.registry(),
        caps(),
        UpgradeOptions {
            conflict_mode: ConflictMode::Abort,
            variables: BTreeMap::from([("greeting".to_owned(), json!("salam"))]),
        },
    )
    .unwrap();
    commit(&fixture.root, &overridden.preview.plan).unwrap();
    assert_eq!(fixture.read("greeting.txt"), "salam\n");
    assert_eq!(
        load_state(&fixture.root).unwrap().variables["greeting"],
        "salam"
    );
    // The override is persistent: a later upgrade keeps it.
    let again = fixture.upgrade(ConflictMode::Abort);
    assert!(!again.preview.plan.has_changes());
}

#[test]
fn s15_missing_recipe_and_downgrade_are_rejected_without_writes() {
    let fixture = Fixture::new(&[]);
    fixture.to_v2();
    fixture.upgrade_apply(ConflictMode::Abort);
    let before = fixture.tree_hash();
    // Registry goes back to an older version than installed.
    write_recipe(
        &fixture.registry_dir,
        "app",
        "0.1.5",
        &[],
        &[],
        &[],
        &[file("package.json", "merge_json", package_json(1))],
    );
    assert!(matches!(
        fixture.try_upgrade(ConflictMode::Abort),
        Err(ScafflareError::Downgrade { .. })
    ));
    fs::remove_dir_all(fixture.registry_dir.join("app")).unwrap();
    assert!(matches!(
        fixture.try_upgrade(ConflictMode::Abort),
        Err(ScafflareError::RecipeNotFound(_))
    ));
    assert_eq!(fixture.tree_hash(), before);
    let status = project_status(&fixture.root, &fixture.registry()).unwrap();
    assert_eq!(status.recipes[0].available, None);
}

fn downgrade_to_legacy_lockfile(fixture: &Fixture) {
    let mut state = load_state(&fixture.root).unwrap();
    for managed in state.managed_files.values_mut() {
        managed.base = None;
    }
    fs::write(
        fixture.path(".scafflare/lock.yaml"),
        serialize_state(&state).unwrap(),
    )
    .unwrap();
    fs::remove_dir_all(fixture.path(".scafflare/base")).unwrap();
}

#[test]
fn s16_version_one_projects_fast_forward_and_degrade_safely() {
    // Untouched files fast-forward even without a base.
    let fixture = Fixture::new(&[]);
    downgrade_to_legacy_lockfile(&fixture);
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(fixture.outcome(&preview, "README.md"), OutcomeKind::Updated);
    assert_eq!(fixture.read("README.md"), "# Demo v2\n\nHello\n");
    assert_state_invariants(&fixture.root); // migrated: every file has a base now

    // Modified text without a base cannot be merged: conflict, never overwrite.
    let fixture = Fixture::new(&[]);
    fixture.write("src/main.ts", &main_ts(&[(2, "USER")]));
    downgrade_to_legacy_lockfile(&fixture);
    fixture.to_v2();
    let preview = fixture.upgrade(ConflictMode::Abort);
    assert_eq!(preview.preview.conflicts[0].kind, ConflictKind::NoBase);
    assert_eq!(fixture.read("src/main.ts"), main_ts(&[(2, "USER")]));

    // Modified JSON without a base merges additively.
    let fixture = Fixture::new(&[]);
    let mut manifest = json_file(&fixture, "package.json");
    manifest["dependencies"]["dayjs"] = json!("^1");
    fixture.write(
        "package.json",
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );
    downgrade_to_legacy_lockfile(&fixture);
    fixture.to_v2();
    let preview = preview_upgrade(
        &fixture.root,
        &fixture.registry(),
        caps(),
        UpgradeOptions::default(),
    )
    .unwrap();
    assert!(
        preview
            .preview
            .conflicts
            .iter()
            .all(|c| c.path != Path::new("package.json")),
        "additive merge must not conflict: {:?}",
        preview.preview.conflicts
    );
}

#[test]
fn s17_crash_at_every_point_converges_to_the_same_tree() {
    fn prepared() -> Fixture {
        let fixture = Fixture::new(&[]);
        fixture.write("src/main.ts", &main_ts(&[(2, "USER line two")]));
        let mut manifest = json_file(&fixture, "package.json");
        manifest["dependencies"]["dayjs"] = json!("^1.0.0");
        fixture.write(
            "package.json",
            &serde_json::to_string_pretty(&manifest).unwrap(),
        );
        fixture.write("stable.txt", "s1\nUSER\ns3\ns4\ns5\ns6\n");
        fixture.to_v2();
        fixture
    }

    let reference = prepared();
    reference.upgrade_apply(ConflictMode::Abort);
    let expected = reference.tree_hash();
    let total = ordered_changes(&prepared().upgrade(ConflictMode::Abort).preview.plan).len();
    assert!(total >= 5);

    for crash_after in 0..=total {
        let fixture = prepared();
        let plan = fixture.upgrade(ConflictMode::Abort).preview.plan;
        apply_plan_limited(&fixture.root, &plan, Some(crash_after)).unwrap();
        let recovery = fixture.upgrade(ConflictMode::Abort);
        assert!(
            !recovery.preview.has_conflicts(),
            "crash_after={crash_after}: {:?}",
            recovery.preview.conflicts
        );
        commit(&fixture.root, &recovery.preview.plan).unwrap();
        assert_eq!(fixture.tree_hash(), expected, "crash_after={crash_after}");
        assert_state_invariants(&fixture.root);
    }
}

#[test]
fn s18_failure_midway_rolls_everything_back() {
    let fixture = Fixture::new(&[]);
    fixture.to_v2();
    fs::create_dir(fixture.path("greeting.txt")).unwrap(); // blocks a write later in the plan
    let before = fixture.tree_hash();
    // Reading a directory as a file fails during analysis: nothing is written.
    assert!(fixture.try_upgrade(ConflictMode::Abort).is_err());
    assert_eq!(fixture.tree_hash(), before);

    // Failure during commit: inject a blocking change after planning.
    let fixture = Fixture::new(&[]);
    fixture.to_v2();
    let mut plan = fixture.upgrade(ConflictMode::Abort).preview.plan;
    fs::create_dir(fixture.path("blocker")).unwrap();
    let before = fixture.tree_hash();
    plan.changes.push(scafflare_core::plan::PlannedChange {
        kind: ChangeKind::Update,
        path: PathBuf::from("blocker"),
        recipe: "test".to_owned(),
        contents: Some(b"x".to_vec()),
    });
    assert!(commit(&fixture.root, &plan).is_err());
    assert_eq!(fixture.tree_hash(), before);
}

#[test]
fn s19_second_upgrade_is_noop_and_add_remove_work_afterwards() {
    let fixture = Fixture::new(&[]);
    fixture.write("src/main.ts", &main_ts(&[(2, "USER line two")]));
    fixture.to_v2();
    fixture.upgrade_apply(ConflictMode::Abort);
    let settled = fixture.tree_hash();
    let second = fixture.upgrade_apply(ConflictMode::Abort);
    assert!(!second.preview.plan.has_changes());
    assert_eq!(fixture.tree_hash(), settled);

    let added = preview_add(
        &fixture.root,
        &fixture.registry(),
        vec!["extra".into()],
        caps(),
    )
    .unwrap();
    commit(&fixture.root, &added.plan).unwrap();
    assert_eq!(fixture.read("extra.txt"), "extra\n");
    assert_state_invariants(&fixture.root);
    assert_eq!(
        fixture.read("src/main.ts"),
        main_ts(&[(2, "USER line two"), (10, "line10 v2")])
    );

    let (plan, _) = preview_remove(&fixture.root, &fixture.registry(), "extra", caps()).unwrap();
    commit(&fixture.root, &plan).unwrap();
    assert!(!fixture.path("extra.txt").exists());
    // Dependencies/scripts added to manifests stay (documented behaviour).
    assert_eq!(
        json_file(&fixture, "package.json")["scripts"]["extra"],
        "node extra"
    );
    assert_state_invariants(&fixture.root);
    let after = fixture.upgrade(ConflictMode::Abort);
    assert!(!after.preview.has_conflicts());
}

#[test]
fn s20_add_merges_into_edited_package_json_and_rejects_outdated_projects() {
    let fixture = Fixture::new(&[]);
    let mut manifest = json_file(&fixture, "package.json");
    manifest["dependencies"]["dayjs"] = json!("^1");
    fixture.write(
        "package.json",
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );
    let added = preview_add(
        &fixture.root,
        &fixture.registry(),
        vec!["extra".into()],
        caps(),
    )
    .unwrap();
    assert!(!added.has_conflicts());
    commit(&fixture.root, &added.plan).unwrap();
    let merged = json_file(&fixture, "package.json");
    assert_eq!(merged["dependencies"]["dayjs"], "^1");
    assert_eq!(merged["scripts"]["extra"], "node extra");
    assert_state_invariants(&fixture.root);

    fixture.to_v2();
    let before = fixture.tree_hash();
    assert!(matches!(
        preview_add(
            &fixture.root,
            &fixture.registry(),
            vec!["other".into()],
            caps()
        ),
        Err(ScafflareError::OutdatedProject { .. })
    ));
    assert!(matches!(
        preview_remove(&fixture.root, &fixture.registry(), "extra", caps()),
        Err(ScafflareError::OutdatedProject { .. })
    ));
    assert_eq!(fixture.tree_hash(), before);
}

#[test]
fn s21_crlf_working_tree_counts_as_unmodified_and_keeps_its_line_endings() {
    let fixture = Fixture::new(&[]);
    // Simulate git autocrlf: identical content with CRLF endings.
    for path in ["src/main.ts", "README.md", "stable.txt"] {
        let converted = fixture.read(path).replace('\n', "\r\n");
        fs::write(fixture.path(path), converted).unwrap();
    }
    assert!(!project_status(&fixture.root, &fixture.registry())
        .unwrap()
        .needs_attention());
    fixture.to_v2();
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert_eq!(fixture.outcome(&preview, "README.md"), OutcomeKind::Updated);
    assert_eq!(fixture.read("README.md"), "# Demo v2\r\n\r\nHello\r\n");
    assert_eq!(
        fixture.outcome(&preview, "stable.txt"),
        OutcomeKind::Unchanged
    );

    // A CRLF file with a user edit merges and stays CRLF.
    let fixture = Fixture::new(&[]);
    let edited = main_ts(&[(2, "USER line two")]).replace('\n', "\r\n");
    fs::write(fixture.path("src/main.ts"), edited).unwrap();
    fixture.to_v2();
    fixture.upgrade_apply(ConflictMode::Abort);
    let merged = fixture.read("src/main.ts");
    assert_eq!(
        merged,
        main_ts(&[(2, "USER line two"), (10, "line10 v2")]).replace('\n', "\r\n")
    );
    assert_state_invariants(&fixture.root);
}

#[cfg(unix)]
#[test]
fn s22_symlinked_path_components_are_rejected() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new(&[]);
    fixture.to_v2();
    let outside = tempfile::tempdir().unwrap();
    // Move src away and replace it with a symlink pointing outside the project.
    fs::rename(fixture.path("src"), outside.path().join("src-real")).unwrap();
    symlink(outside.path().join("src-real"), fixture.path("src")).unwrap();
    let preview = fixture.upgrade(ConflictMode::Abort);
    let result = commit(&fixture.root, &preview.preview.plan);
    assert!(matches!(result, Err(ScafflareError::Transaction { .. })));
    assert_eq!(
        fs::read_to_string(outside.path().join("src-real/main.ts")).unwrap(),
        main_ts(&[])
    );
}

#[test]
fn s24_status_reports_outdated_modified_missing_and_legacy_files() {
    let fixture = Fixture::new(&[]);
    let clean = project_status(&fixture.root, &fixture.registry()).unwrap();
    assert!(!clean.needs_attention());
    assert_eq!(clean.legacy_files, 0);

    fixture.write("README.md", "changed\n");
    fs::remove_file(fixture.path("legacy.txt")).unwrap();
    fixture.to_v2();
    let status = project_status(&fixture.root, &fixture.registry()).unwrap();
    assert!(status.outdated);
    assert_eq!((status.modified, status.missing), (1, 1));
    assert_eq!(status.recipes[0].installed.to_string(), "0.1.0");
    assert_eq!(
        status.recipes[0].available.as_ref().unwrap().to_string(),
        "0.2.0"
    );

    downgrade_to_legacy_lockfile(&fixture);
    let legacy = project_status(&fixture.root, &fixture.registry()).unwrap();
    assert_eq!(legacy.legacy_files, legacy.files.len());
}

#[test]
fn s25_corrupt_or_missing_blob_degrades_to_no_base_never_panics() {
    let fixture = Fixture::new(&[]);
    for entry in fs::read_dir(fixture.path(".scafflare/base")).unwrap() {
        fs::write(entry.unwrap().path(), b"corrupt").unwrap();
    }
    fixture.to_v2();
    // Untouched files still fast-forward through the sha256 fallback.
    let preview = fixture.upgrade_apply(ConflictMode::Abort);
    assert!(!preview.preview.has_conflicts());
    assert_eq!(fixture.read("README.md"), "# Demo v2\n\nHello\n");
    assert_state_invariants(&fixture.root);
}

#[test]
fn s26_hand_edited_lockfile_cannot_escape_the_project() {
    let fixture = Fixture::new(&[]);
    let lock = fs::read_to_string(fixture.path(".scafflare/lock.yaml")).unwrap();
    fs::write(
        fixture.path(".scafflare/lock.yaml"),
        lock.replace("README.md:", "../outside.txt:"),
    )
    .unwrap();
    assert!(matches!(
        fixture.try_upgrade(ConflictMode::Abort),
        Err(ScafflareError::InvalidState(_))
    ));
}

// ---------------------------------------------------------------------------
// Property: random operation sequences never break the invariants
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Op {
    EditMain(usize),
    EditPackage(u8),
    EditStable,
    DeleteReadme,
    GoV2,
    UpgradeAbort,
    UpgradeMarkers,
    AddExtra,
    RemoveExtra,
    Resolve,
}

fn arb_op() -> impl proptest::strategy::Strategy<Value = Op> {
    use proptest::prelude::*;
    prop_oneof![
        (1usize..=12).prop_map(Op::EditMain),
        (0u8..4).prop_map(Op::EditPackage),
        Just(Op::EditStable),
        Just(Op::DeleteReadme),
        Just(Op::GoV2),
        Just(Op::UpgradeAbort),
        Just(Op::UpgradeMarkers),
        Just(Op::AddExtra),
        Just(Op::RemoveExtra),
        Just(Op::Resolve),
    ]
}

fn resolve_conflicts(fixture: &Fixture) {
    let state = load_state(&fixture.root).unwrap();
    for path in state.managed_files.keys() {
        let sidecar = fixture.path(&format!("{path}.scafflare-new"));
        if sidecar.exists() {
            fs::remove_file(sidecar).unwrap();
        }
        if let Ok(text) = fs::read_to_string(fixture.path(path)) {
            if text.contains("<<<<<<<") {
                let kept: Vec<&str> = text
                    .lines()
                    .filter(|l| {
                        !(l.starts_with("<<<<<<<")
                            || l.starts_with("=======")
                            || l.starts_with(">>>>>>>")
                            || l.starts_with("|||||||"))
                    })
                    .collect();
                fs::write(fixture.path(path), kept.join("\n") + "\n").unwrap();
            }
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(48)
    ))]

    #[test]
    fn s23_random_operation_sequences_preserve_invariants(
        ops in proptest::collection::vec(arb_op(), 1..14)
    ) {
        let fixture = Fixture::new(&[]);
        let mut v2 = false;
        for op in ops {
            match op {
                Op::EditMain(line) => {
                    if let Ok(text) = fs::read_to_string(fixture.path("src/main.ts")) {
                        if !text.contains("<<<<<<<") {
                            let edited: Vec<String> = text.lines().enumerate()
                                .map(|(i, l)| if i + 1 == line { format!("user{line}") } else { l.to_owned() })
                                .collect();
                            fixture.write("src/main.ts", &(edited.join("\n") + "\n"));
                        }
                    }
                }
                Op::EditPackage(n) => {
                    if let Ok(text) = fs::read_to_string(fixture.path("package.json")) {
                        if let Ok(mut manifest) = serde_json::from_str::<Value>(&text) {
                            manifest["dependencies"][format!("dep{n}")] = json!("^1");
                            fixture.write("package.json", &serde_json::to_string_pretty(&manifest).unwrap());
                        }
                    }
                }
                Op::EditStable => fixture.write("stable.txt", "s1\nUSER\ns3\ns4\ns5\ns6\n"),
                Op::DeleteReadme => { let _ = fs::remove_file(fixture.path("README.md")); }
                Op::GoV2 => if !v2 { fixture.to_v2(); v2 = true; },
                Op::UpgradeAbort | Op::UpgradeMarkers => {
                    let mode = if matches!(op, Op::UpgradeAbort) { ConflictMode::Abort } else { ConflictMode::Markers };
                    if let Ok(preview) = fixture.try_upgrade(mode) {
                        let before = fixture.tree_hash();
                        commit(&fixture.root, &preview.preview.plan).unwrap();
                        if preview.preview.has_conflicts() && mode == ConflictMode::Abort {
                            proptest::prop_assert_eq!(fixture.tree_hash(), before);
                        } else {
                            assert_state_invariants(&fixture.root);
                        }
                    }
                }
                Op::AddExtra => {
                    if let Ok(preview) = preview_add_with(&fixture.root, &fixture.registry(), vec!["extra".into()], caps(), ConflictMode::Abort) {
                        let before = fixture.tree_hash();
                        commit(&fixture.root, &preview.plan).unwrap();
                        if preview.has_conflicts() {
                            proptest::prop_assert_eq!(fixture.tree_hash(), before);
                        } else {
                            assert_state_invariants(&fixture.root);
                        }
                    }
                }
                Op::RemoveExtra => {
                    if let Ok((plan, _)) = preview_remove(&fixture.root, &fixture.registry(), "extra", caps()) {
                        commit(&fixture.root, &plan).unwrap();
                        assert_state_invariants(&fixture.root);
                    }
                }
                Op::Resolve => resolve_conflicts(&fixture),
            }
        }
    }
}
