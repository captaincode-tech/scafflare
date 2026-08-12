use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, ScafflareError};

pub const LOCKFILE_VERSION: u32 = 1;

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
    pub sha256: String,
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
    if state.lockfile_version != LOCKFILE_VERSION {
        return Err(ScafflareError::InvalidState(format!(
            "unsupported lockfile_version `{}`",
            state.lockfile_version
        )));
    }
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

    #[test]
    fn serializes_versioned_state() {
        let state = ProjectState::new("example".to_owned(), BTreeMap::new());
        let output = String::from_utf8(serialize_state(&state).unwrap()).unwrap();
        assert!(output.contains("lockfile_version: 1"));
    }
}
