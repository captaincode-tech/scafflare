use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::error::{Result, ScafflareError};
use crate::recipe::FileStrategy;
use crate::render::RenderedFile;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Create,
    Update,
    Delete,
    Unchanged,
    Skip,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedChange {
    pub kind: ChangeKind,
    pub path: PathBuf,
    pub recipe: String,
    #[serde(skip_serializing)]
    pub contents: Option<Vec<u8>>,
}

impl PlannedChange {
    pub fn needs_confirmation(&self) -> bool {
        !matches!(self.kind, ChangeKind::Unchanged | ChangeKind::Skip)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FilePlan {
    pub changes: Vec<PlannedChange>,
}

impl FilePlan {
    pub fn has_changes(&self) -> bool {
        self.changes.iter().any(PlannedChange::needs_confirmation)
    }

    pub fn summary(&self) -> BTreeMap<ChangeKind, usize> {
        let mut summary = BTreeMap::new();
        for change in &self.changes {
            *summary.entry(change.kind).or_insert(0) += 1;
        }
        summary
    }
}

pub fn plan_files(root: &Path, rendered: &[RenderedFile]) -> Result<FilePlan> {
    let mut desired = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut owners = BTreeMap::<PathBuf, String>::new();
    let mut plan = FilePlan::default();

    for file in rendered {
        let destination = root.join(&file.destination);
        let existing = desired
            .get(&file.destination)
            .cloned()
            .or_else(|| fs::read(&destination).ok());
        let target_exists = existing.is_some();

        match file.strategy {
            FileStrategy::Skip => {
                plan.changes.push(PlannedChange {
                    kind: ChangeKind::Skip,
                    path: file.destination.clone(),
                    recipe: file.recipe.clone(),
                    contents: None,
                });
            }
            FileStrategy::Create => {
                if let Some(current) = existing {
                    if current != file.contents {
                        return Err(ScafflareError::FileConflict {
                            path: file.destination.clone(),
                            reason: format!(
                                "recipe `{}` uses create but the file already exists",
                                file.recipe
                            ),
                        });
                    }
                    plan.changes.push(PlannedChange {
                        kind: ChangeKind::Unchanged,
                        path: file.destination.clone(),
                        recipe: file.recipe.clone(),
                        contents: None,
                    });
                } else {
                    desired.insert(file.destination.clone(), file.contents.clone());
                    owners.insert(file.destination.clone(), file.recipe.clone());
                }
            }
            FileStrategy::Fail => {
                if target_exists {
                    return Err(ScafflareError::FileConflict {
                        path: file.destination.clone(),
                        reason: format!("recipe `{}` requires this path not to exist", file.recipe),
                    });
                }
                desired.insert(file.destination.clone(), file.contents.clone());
                owners.insert(file.destination.clone(), file.recipe.clone());
            }
            FileStrategy::Replace => {
                let kind = if target_exists {
                    ChangeKind::Update
                } else {
                    ChangeKind::Create
                };
                if existing.as_deref() == Some(file.contents.as_slice()) {
                    plan.changes.push(PlannedChange {
                        kind: ChangeKind::Unchanged,
                        path: file.destination.clone(),
                        recipe: file.recipe.clone(),
                        contents: None,
                    });
                } else {
                    desired.insert(file.destination.clone(), file.contents.clone());
                    owners.insert(file.destination.clone(), file.recipe.clone());
                    plan.changes.push(PlannedChange {
                        kind,
                        path: file.destination.clone(),
                        recipe: file.recipe.clone(),
                        contents: None,
                    });
                }
            }
            FileStrategy::MergeJson => {
                let left = match existing {
                    Some(ref bytes) => parse_json(&file.destination, bytes)?,
                    None => Value::Object(Default::default()),
                };
                let right = parse_json(&file.destination, &file.contents)?;
                let merged = deep_merge(left, right);
                let contents = serde_json::to_vec_pretty(&merged)
                    .map_err(|error| ScafflareError::Serialization(error.to_string()))?
                    .into_iter()
                    .chain(std::iter::once(b'\n'))
                    .collect::<Vec<_>>();
                if existing.as_deref() == Some(contents.as_slice()) {
                    plan.changes.push(PlannedChange {
                        kind: ChangeKind::Unchanged,
                        path: file.destination.clone(),
                        recipe: file.recipe.clone(),
                        contents: None,
                    });
                } else {
                    desired.insert(file.destination.clone(), contents);
                    owners.insert(file.destination.clone(), file.recipe.clone());
                    plan.changes.push(PlannedChange {
                        kind: if target_exists {
                            ChangeKind::Update
                        } else {
                            ChangeKind::Create
                        },
                        path: file.destination.clone(),
                        recipe: file.recipe.clone(),
                        contents: None,
                    });
                }
            }
        }
    }

    // A composed target may be touched by several recipes (notably package.json).
    // Keep only one terminal write per path, containing the fully merged result.
    plan.changes
        .retain(|change| !matches!(change.kind, ChangeKind::Create | ChangeKind::Update));
    for (path, contents) in desired {
        let target = root.join(&path);
        let kind = match fs::read(&target) {
            Ok(existing) if existing == contents => ChangeKind::Unchanged,
            Ok(_) => ChangeKind::Update,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => ChangeKind::Create,
            Err(error) => return Err(ScafflareError::io(&target, error)),
        };
        let recipe = owners.remove(&path).unwrap_or_else(|| "unknown".to_owned());
        plan.changes.push(PlannedChange {
            kind,
            path,
            recipe,
            contents: if kind == ChangeKind::Unchanged {
                None
            } else {
                Some(contents)
            },
        });
    }

    for change in &mut plan.changes {
        if change.contents.is_none()
            && matches!(change.kind, ChangeKind::Create | ChangeKind::Update)
        {
            if let Some(contents) = desired_contents(root, rendered, &change.path)? {
                change.contents = Some(contents);
            }
        }
    }
    plan.changes.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.recipe.cmp(&right.recipe))
    });
    Ok(plan)
}

fn desired_contents(
    root: &Path,
    rendered: &[RenderedFile],
    target: &Path,
) -> Result<Option<Vec<u8>>> {
    let mut value = fs::read(root.join(target)).ok();
    for file in rendered.iter().filter(|file| file.destination == target) {
        match file.strategy {
            FileStrategy::Skip => {}
            FileStrategy::Create | FileStrategy::Fail | FileStrategy::Replace => {
                value = Some(file.contents.clone())
            }
            FileStrategy::MergeJson => {
                let left = match value {
                    Some(bytes) => parse_json(target, &bytes)?,
                    None => Value::Object(Default::default()),
                };
                let merged = deep_merge(left, parse_json(target, &file.contents)?);
                value = Some(
                    serde_json::to_vec_pretty(&merged)
                        .map_err(|error| ScafflareError::Serialization(error.to_string()))?
                        .into_iter()
                        .chain(std::iter::once(b'\n'))
                        .collect(),
                );
            }
        }
    }
    Ok(value)
}

fn parse_json(path: &Path, bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice(bytes).map_err(|error| ScafflareError::FileConflict {
        path: path.to_path_buf(),
        reason: format!("valid JSON is required for merge_json: {error}"),
    })
}

pub fn deep_merge(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Object(mut left), Value::Object(right)) => {
            for (key, right_value) in right {
                let merged = match left.remove(&key) {
                    Some(left_value) => deep_merge(left_value, right_value),
                    None => right_value,
                };
                left.insert(key, merged);
            }
            Value::Object(left)
        }
        (_, right) => right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::RenderedFile;

    #[test]
    fn deeply_merges_json_objects_without_string_injection() {
        let left = serde_json::json!({"scripts": {"test": "vitest"}, "name": "app"});
        let right = serde_json::json!({"scripts": {"lint": "biome check ."}, "type": "module"});
        assert_eq!(
            deep_merge(left, right),
            serde_json::json!({"scripts": {"test": "vitest", "lint": "biome check ."}, "name": "app", "type": "module"})
        );
    }

    #[test]
    fn is_idempotent_for_existing_create_content() {
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("same.txt"), "same").unwrap();
        let plan = plan_files(
            temporary.path(),
            &[RenderedFile {
                recipe: "test".to_owned(),
                destination: PathBuf::from("same.txt"),
                strategy: FileStrategy::Create,
                contents: b"same".to_vec(),
            }],
        )
        .unwrap();
        assert_eq!(plan.changes[0].kind, ChangeKind::Unchanged);
    }
}
