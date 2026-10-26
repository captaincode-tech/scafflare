use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, ScafflareError};
use crate::recipe::validate_relative_path;

/// Version written by this release. Version 1 lockfiles are still readable.
pub const LOCKFILE_VERSION: u32 = 2;
pub const MIN_SUPPORTED_LOCKFILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectState {
    pub lockfile_version: u32,
    pub project_name: String,
    pub recipes: Vec<InstalledRecipe>,
    pub variables: BTreeMap<String, Value>,
    #[serde(default)]
    pub managed_files: BTreeMap<String, ManagedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InstalledRecipe {
    pub name: String,
    pub version: Version,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedFile {
    pub recipes: Vec<String>,
    /// Hash of the file content (line endings normalized) as it was left on disk
    /// by the last Scafflare operation. Used to detect user drift.
    pub sha256: String,
    /// Hash of the pure template render (merge ancestor) stored in `.scafflare/base/`.
    /// `None` only for projects created by lockfile version 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

impl ProjectState {
    pub fn new(project_name: String, variables: BTreeMap<String, Value>) -> Self {
        Self {
            lockfile_version: LOCKFILE_VERSION,
            project_name,
            recipes: Vec::new(),
            variables,
            managed_files: BTreeMap::new(),
        }
    }

    pub fn has_recipe(&self, name: &str) -> bool {
        self.recipes.iter().any(|recipe| recipe.name == name)
    }

    pub fn remove_recipe(&mut self, name: &str) -> bool {
        let before = self.recipes.len();
        self.recipes.retain(|recipe| recipe.name != name);
        for file in self.managed_files.values_mut() {
            file.recipes.retain(|owner| owner != name);
        }
        self.managed_files
            .retain(|_, file| !file.recipes.is_empty());
        before != self.recipes.len()
    }
}

pub fn lockfile_path(root: &Path) -> PathBuf {
    root.join(".scafflare").join("lock.yaml")
}

pub fn load_state(root: &Path) -> Result<ProjectState> {
    let path = lockfile_path(root);
    let input = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ScafflareError::MissingState(path.clone())
        } else {
            ScafflareError::io(&path, error)
        }
    })?;
    let state: ProjectState = serde_yaml::from_str(&input)
        .map_err(|error| ScafflareError::InvalidState(error.to_string()))?;
    let mut state = state;
    if !(MIN_SUPPORTED_LOCKFILE_VERSION..=LOCKFILE_VERSION).contains(&state.lockfile_version) {
        return Err(ScafflareError::InvalidState(format!(
            "unsupported lockfile_version `{}`",
            state.lockfile_version
        )));
    }
    for (path, file) in &state.managed_files {
        validate_relative_path("lockfile", path).map_err(|_| {
            ScafflareError::InvalidState(format!("unsafe managed file path `{path}`"))
        })?;
        if let Some(base) = &file.base {
            if !crate::blobs::is_valid_hash(base) {
                return Err(ScafflareError::InvalidState(format!(
                    "invalid base hash for `{path}`"
                )));
            }
        }
    }
    // Version 1 projects are migrated in memory; every write produces version 2.
    state.lockfile_version = LOCKFILE_VERSION;
    Ok(state)
}

pub fn serialize_state(state: &ProjectState) -> Result<Vec<u8>> {
    let mut output = serde_yaml::to_string(state)
        .map_err(|error| ScafflareError::Serialization(error.to_string()))?;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_lock(root: &Path, body: &str) {
        fs::create_dir_all(root.join(".scafflare")).unwrap();
        fs::write(lockfile_path(root), body).unwrap();
    }

    #[test]
    fn migrates_version_one_lockfile_in_memory() {
        let temporary = tempfile::tempdir().unwrap();
        write_lock(
            temporary.path(),
            "lockfile_version: 1\nproject_name: a\nrecipes: []\nvariables: {}\nmanaged_files:\n  package.json:\n    recipes: [node]\n    sha256: abc\n",
        );
        let state = load_state(temporary.path()).unwrap();
        assert_eq!(state.lockfile_version, 2);
        assert!(state.managed_files["package.json"].base.is_none());
        let output = String::from_utf8(serialize_state(&state).unwrap()).unwrap();
        assert!(output.contains("lockfile_version: 2"));
        assert!(!output.contains("base:"));
    }

    #[test]
    fn rejects_future_and_zero_versions() {
        let temporary = tempfile::tempdir().unwrap();
        for version in [0, 3] {
            write_lock(
                temporary.path(),
                &format!(
                    "lockfile_version: {version}\nproject_name: a\nrecipes: []\nvariables: {{}}\n"
                ),
            );
            assert!(matches!(
                load_state(temporary.path()),
                Err(ScafflareError::InvalidState(_))
            ));
        }
    }

    #[test]
    fn rejects_unsafe_managed_paths_and_bad_base_hashes() {
        let temporary = tempfile::tempdir().unwrap();
        write_lock(
            temporary.path(),
            "lockfile_version: 2\nproject_name: a\nrecipes: []\nvariables: {}\nmanaged_files:\n  ../escape:\n    recipes: [x]\n    sha256: abc\n",
        );
        assert!(load_state(temporary.path()).is_err());
        write_lock(
            temporary.path(),
            "lockfile_version: 2\nproject_name: a\nrecipes: []\nvariables: {}\nmanaged_files:\n  a.txt:\n    recipes: [x]\n    sha256: abc\n    base: ../../etc/passwd\n",
        );
        assert!(load_state(temporary.path()).is_err());
    }

    #[test]
    fn roundtrips_version_two_state_with_base() {
        let temporary = tempfile::tempdir().unwrap();
        let mut state = ProjectState::new("a".to_owned(), BTreeMap::new());
        state.managed_files.insert(
            "a.txt".to_owned(),
            ManagedFile {
                recipes: vec!["r".to_owned()],
                sha256: "0".repeat(64),
                base: Some("f".repeat(64)),
            },
        );
        write_lock(
            temporary.path(),
            &String::from_utf8(serialize_state(&state).unwrap()).unwrap(),
        );
        let loaded = load_state(temporary.path()).unwrap();
        assert_eq!(loaded.managed_files["a.txt"].base, Some("f".repeat(64)));
    }

    #[test]
    fn parses_shipped_version_one_example_lockfile() {
        let example =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/clean-express-todo");
        let state = load_state(&example).unwrap();
        assert_eq!(state.lockfile_version, 2);
        assert!(state.managed_files.values().all(|file| file.base.is_none()));
    }

    #[test]
    fn serializes_versioned_state() {
        let state = ProjectState::new("example".to_owned(), BTreeMap::new());
        let output = String::from_utf8(serialize_state(&state).unwrap()).unwrap();
        assert!(output.contains("lockfile_version: 2"));
    }
}
