mod common;

use std::fs;

use predicates::prelude::*;
use serde_json::Value;

#[test]
fn list_json_contains_official_recipes() {
    let output = common::scafflare()
        .args(["--json", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let names: Vec<&str> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    for expected in ["node", "typescript", "express", "hono", "drizzle", "pino"] {
        assert!(names.contains(&expected), "{expected} missing");
    }
}

#[test]
fn init_json_requires_yes() {
    let temporary = tempfile::tempdir().unwrap();
    common::scafflare()
        .args(["--json", "init", "app", "--directory"])
        .arg(temporary.path())
        .args(["--non-interactive", "--set", "framework=none"])
        .assert()
        .failure();
    assert!(!temporary.path().join("app").exists());
}

#[test]
fn init_json_emits_machine_readable_envelope() {
    let temporary = tempfile::tempdir().unwrap();
    let output = common::scafflare()
        .args(["--json", "init", "app", "--directory"])
        .arg(temporary.path())
        .args(["--non-interactive", "--yes", "--set", "framework=none"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert!(value["plan"]["changes"].is_array());
    assert!(temporary.path().join("app/.scafflare/lock.yaml").is_file());
}

#[test]
fn init_refuses_non_empty_target() {
    let temporary = tempfile::tempdir().unwrap();
    fs::create_dir(temporary.path().join("app")).unwrap();
    fs::write(temporary.path().join("app/keep.txt"), "x").unwrap();
    common::scafflare()
        .args(["init", "app", "--directory"])
        .arg(temporary.path())
        .args(["--non-interactive", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("must be empty"));
}

#[test]
fn validate_add_remove_roundtrip() {
    let temporary = tempfile::tempdir().unwrap();
    common::init_combo(
        temporary.path(),
        "app",
        &["framework=none", "architecture=minimal", "database=none"],
    );
    let root = temporary.path().join("app");

    common::scafflare()
        .args(["validate", "--directory"])
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("valid"));

    common::scafflare()
        .args(["add", "pino", "--yes", "--quiet", "--directory"])
        .arg(&root)
        .assert()
        .success();
    assert!(root.join("src/logger.ts").is_file());

    common::scafflare()
        .args(["remove", "pino", "--yes", "--quiet", "--directory"])
        .arg(&root)
        .assert()
        .success();
    assert!(!root.join("src/logger.ts").exists());
}

#[test]
fn validate_fails_outside_managed_project() {
    let temporary = tempfile::tempdir().unwrap();
    common::scafflare()
        .args(["validate", "--directory"])
        .arg(temporary.path())
        .assert()
        .failure();
}

#[test]
fn recipe_validate_accepts_official_recipes() {
    let recipes = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes/official");
    for entry in fs::read_dir(recipes).unwrap() {
        let directory = entry.unwrap().path();
        if !directory.is_dir() {
            continue; // e.g. the CHECKSUMS manifest
        }
        let path = directory.join("recipe.yaml");
        common::scafflare()
            .args(["recipe", "validate"])
            .arg(&path)
            .assert()
            .success();
    }
}
