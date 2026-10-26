#![allow(dead_code)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use sha2::{Digest, Sha256};

pub fn scafflare() -> Command {
    Command::cargo_bin("scafflare").expect("scafflare binary")
}

/// The six combinations also exercised by `scripts/verify-fixtures.sh`.
pub fn combos() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "node-minimal",
            vec![
                "framework=none",
                "architecture=minimal",
                "database=none",
                "biome_enabled=true",
                "vitest_enabled=true",
            ],
        ),
        (
            "express-minimal",
            vec![
                "framework=express",
                "architecture=minimal",
                "database=none",
                "biome_enabled=true",
                "vitest_enabled=true",
            ],
        ),
        (
            "express-layered",
            vec![
                "framework=express",
                "architecture=layered",
                "database=none",
                "biome_enabled=true",
                "vitest_enabled=true",
            ],
        ),
        (
            "hono-layered",
            vec![
                "framework=hono",
                "architecture=layered",
                "database=none",
                "biome_enabled=true",
                "vitest_enabled=true",
            ],
        ),
        (
            "express-clean-sqlite",
            vec![
                "framework=express",
                "architecture=clean",
                "database=sqlite",
                "biome_enabled=true",
                "vitest_enabled=true",
                "pino_enabled=true",
            ],
        ),
        (
            "hono-clean-sqlite",
            vec![
                "framework=hono",
                "architecture=clean",
                "database=sqlite",
                "biome_enabled=true",
                "vitest_enabled=true",
                "pino_enabled=true",
            ],
        ),
    ]
}

pub fn init_combo(parent: &Path, name: &str, sets: &[&str]) {
    let mut command = scafflare();
    command
        .args(["init", name, "--directory"])
        .arg(parent)
        .args(["--non-interactive", "--yes", "--quiet"]);
    for set in sets {
        command.args(["--set", set]);
    }
    command.assert().success();
}

fn normalized(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    text.into_bytes()
}

/// Deterministic `path sha256` listing of a project tree, excluding `.scafflare`.
pub fn tree_listing(root: &Path) -> String {
    let mut lines = Vec::new();
    for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(root).unwrap();
        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative.starts_with(".scafflare/") {
            continue;
        }
        let contents = normalized(&fs::read(entry.path()).unwrap());
        lines.push(format!("{relative} {:x}", Sha256::digest(&contents)));
    }
    lines.sort();
    lines.join("\n") + "\n"
}

/// Hash of every byte in the tree (including `.scafflare`), for "nothing changed" checks.
pub fn full_tree_hash(root: &Path) -> String {
    let mut hasher = Sha256::new();
    for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let relative = entry.path().strip_prefix(root).unwrap();
            hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
            hasher.update(fs::read(entry.path()).unwrap());
        }
    }
    format!("{:x}", hasher.finalize())
}
