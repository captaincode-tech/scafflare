mod common;

use std::fs;
use std::path::{Path, PathBuf};

use predicates::prelude::*;
use serde_json::Value;

fn write_recipe(registry: &Path, version: u32) {
    let dir = registry.join("app");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("templates")).unwrap();
    let (semver, readme, main) = if version == 1 {
        (
            "0.1.0",
            "# App\n",
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n",
        )
    } else {
        (
            "0.2.0",
            "# App v2\n",
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\nEIGHT\n",
        )
    };
    fs::write(dir.join("templates/readme.jinja"), format!("{readme}\n")).unwrap();
    fs::write(dir.join("templates/main.jinja"), format!("{main}\n")).unwrap();
    fs::write(
        dir.join("recipe.yaml"),
        format!(
            "schema_version: 1\nmetadata:\n  name: app\n  version: {semver}\n  description: test app\nfiles:\n  - source: templates/readme.jinja\n    destination: README.md\n    strategy: create\n  - source: templates/main.jinja\n    destination: main.txt\n    strategy: create\n"
        ),
    )
    .unwrap();
}

struct Project {
    _dir: tempfile::TempDir,
    registry: PathBuf,
    root: PathBuf,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let registry = dir.path().join("registry");
        fs::create_dir_all(&registry).unwrap();
        write_recipe(&registry, 1);
        let project = Self {
            root: dir.path().join("proj"),
            registry,
            _dir: dir,
        };
        let parent = project.root.parent().unwrap().to_owned();
        common::scafflare()
            .arg("--recipe-root")
            .arg(&project.registry)
            .args(["init", "proj", "--directory"])
            .arg(parent)
            .args(["--non-interactive", "--yes", "--quiet", "--recipe", "app"])
            .assert()
            .success();
        project
    }

    fn cmd(&self, args: &[&str]) -> assert_cmd::Command {
        let mut command = common::scafflare();
        command.arg("--recipe-root").arg(&self.registry);
        command.args(args);
        command.arg("--directory").arg(&self.root);
        command
    }

    fn to_v2(&self) {
        write_recipe(&self.registry, 2);
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.root.join(name)).unwrap()
    }
}

#[test]
fn status_is_clean_then_reports_outdated_with_exit_code() {
    let project = Project::new();
    project
        .cmd(&["status", "--exit-code"])
        .assert()
        .success()
        .stdout(predicate::str::contains("clean"));
    project.to_v2();
    project
        .cmd(&["status", "--exit-code"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("outdated"));
    // Without --exit-code the command only reports.
    project.cmd(&["status"]).assert().success();
}

#[test]
fn status_json_has_stable_schema() {
    let project = Project::new();
    project.to_v2();
    let output = project.cmd(&["--json", "status"]).output().unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["outdated"], true);
    assert_eq!(value["recipes"][0]["state"], "outdated");
    assert_eq!(value["recipes"][0]["available"], "0.2.0");
    assert!(value["files"].is_array());
}

#[test]
fn validate_points_to_upgrade_when_outdated() {
    let project = Project::new();
    project.to_v2();
    project
        .cmd(&["validate"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("scafflare upgrade"));
}

#[test]
fn dry_run_writes_nothing_and_diff_shows_changes() {
    let project = Project::new();
    project.to_v2();
    let before = common::full_tree_hash(&project.root);
    project
        .cmd(&["upgrade", "--dry-run", "--diff"])
        .assert()
        .success()
        .stdout(predicate::str::contains("+EIGHT").and(predicate::str::contains("dry run")));
    assert_eq!(common::full_tree_hash(&project.root), before);
}

#[test]
fn upgrade_applies_then_is_idempotent() {
    let project = Project::new();
    project.to_v2();
    project
        .cmd(&["upgrade", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("upgraded app 0.1.0 -> 0.2.0"));
    assert_eq!(project.read("README.md"), "# App v2\n");
    project
        .cmd(&["upgrade", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already up to date"));
    project.cmd(&["validate"]).assert().success();
    project.cmd(&["status", "--exit-code"]).assert().success();
}

#[test]
fn json_upgrade_requires_yes_and_emits_envelope() {
    let project = Project::new();
    project.to_v2();
    let before = common::full_tree_hash(&project.root);
    project.cmd(&["--json", "upgrade"]).assert().failure();
    assert_eq!(common::full_tree_hash(&project.root), before);

    let output = project
        .cmd(&["--json", "upgrade", "--yes"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], false);
    assert_eq!(value["from"][0]["version"], "0.1.0");
    assert_eq!(value["to"][0]["version"], "0.2.0");
    assert!(value["plan"]["changes"].is_array());
    assert!(value["outcomes"].is_array());
    assert!(value["conflicts"].as_array().unwrap().is_empty());
}

#[test]
fn user_edits_are_merged_into_upgrade() {
    let project = Project::new();
    fs::write(
        project.root.join("main.txt"),
        "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n",
    )
    .unwrap();
    project.to_v2();
    project.cmd(&["upgrade", "--yes"]).assert().success();
    assert_eq!(
        project.read("main.txt"),
        "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\nEIGHT\n"
    );
}

#[test]
fn conflict_exits_2_and_leaves_tree_untouched_in_abort_mode() {
    let project = Project::new();
    fs::write(
        project.root.join("main.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\nMINE\n",
    )
    .unwrap();
    project.to_v2();
    let before = common::full_tree_hash(&project.root);
    project
        .cmd(&["upgrade", "--yes"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("main.txt"));
    assert_eq!(common::full_tree_hash(&project.root), before);

    let output = project
        .cmd(&["--json", "upgrade", "--yes"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["conflicts"][0]["path"], "main.txt");
    assert_eq!(value["conflicts"][0]["kind"], "both_modified");
    assert_eq!(common::full_tree_hash(&project.root), before);
}

#[test]
fn markers_mode_writes_markers_status_flags_them_and_upgrade_refuses_until_resolved() {
    let project = Project::new();
    fs::write(
        project.root.join("main.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\nMINE\n",
    )
    .unwrap();
    project.to_v2();
    project
        .cmd(&["upgrade", "--yes", "--conflicts", "markers"])
        .assert()
        .code(2);
    let merged = project.read("main.txt");
    assert!(merged.contains("<<<<<<<") && merged.contains("MINE") && merged.contains("EIGHT"));
    project
        .cmd(&["status", "--exit-code"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("conflicted"));
    project
        .cmd(&["upgrade", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unresolved conflicts"));
    fs::write(project.root.join("main.txt"), "resolved\n").unwrap();
    project.cmd(&["upgrade", "--yes"]).assert().success();
    assert_eq!(project.read("main.txt"), "resolved\n");
}

#[test]
fn set_overrides_are_validated_as_key_value() {
    let project = Project::new();
    project
        .cmd(&["upgrade", "--set", "not-a-pair"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("KEY=VALUE"));
}

#[test]
fn upgrade_outside_a_managed_project_fails_cleanly() {
    let empty = tempfile::tempdir().unwrap();
    common::scafflare()
        .args(["upgrade", "--directory"])
        .arg(empty.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not managed by Scafflare"));
}

#[test]
fn legacy_official_project_upgrades_to_current_node_recipe() {
    let legacy =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/upgrade/legacy-recipes");
    let temporary = tempfile::tempdir().unwrap();
    common::scafflare()
        .arg("--recipe-root")
        .arg(&legacy)
        .args(["init", "legacy-api", "--directory"])
        .arg(temporary.path())
        .args([
            "--non-interactive",
            "--yes",
            "--quiet",
            "--set",
            "framework=express",
            "--set",
            "architecture=clean",
            "--set",
            "database=sqlite",
            "--set",
            "vitest_enabled=true",
        ])
        .assert()
        .success();
    let root = temporary.path().join("legacy-api");
    let lock = fs::read_to_string(root.join(".scafflare/lock.yaml")).unwrap();
    assert!(lock.contains("name: node\n  version: 0.1.0"), "{lock}");

    // Simulate `npm install dayjs` plus a hand edit.
    let manifest_path = root.join("package.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["dependencies"]["dayjs"] = Value::String("^1.11.0".into());
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let server = fs::read_to_string(root.join("src/server.ts")).unwrap();
    fs::write(
        root.join("src/server.ts"),
        format!("// my custom header\n{server}"),
    )
    .unwrap();

    common::scafflare()
        .args(["status", "--exit-code", "--directory"])
        .arg(&root)
        .assert()
        .code(2)
        .stdout(predicate::str::contains("node 0.1.0: outdated"));
    common::scafflare()
        .args(["upgrade", "--yes", "--directory"])
        .arg(&root)
        .assert()
        .success();

    let upgraded: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(upgraded["dependencies"]["dayjs"], "^1.11.0");
    assert_eq!(upgraded["devDependencies"]["typescript"], "^7.0.2");
    assert_eq!(upgraded["engines"]["node"], ">=22.13.0");
    assert!(fs::read_to_string(root.join("src/server.ts"))
        .unwrap()
        .starts_with("// my custom header\n"));
    let lock = fs::read_to_string(root.join(".scafflare/lock.yaml")).unwrap();
    assert!(lock.contains("name: node\n  version: 0.2.0"), "{lock}");
    common::scafflare()
        .args(["upgrade", "--yes", "--directory"])
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("already up to date"));
}
