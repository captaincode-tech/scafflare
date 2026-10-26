use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::blobs::normalize_eol;
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

/// One path of the pure template render ("theirs" in a three-way merge).
#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub contents: Vec<u8>,
    /// Recipes that contributed to this path (sorted, unique, never `skip`).
    pub owners: Vec<String>,
    /// Strategy of the last contributing recipe.
    pub strategy: FileStrategy,
    /// True when the final content is a structured JSON merge result.
    pub json: bool,
}

/// Render every recipe file into an in-memory tree without consulting the disk.
///
/// Several recipes may contribute to one path (notably `package.json`); the
/// result contains the single fully merged content.
pub fn render_tree(rendered: &[RenderedFile]) -> Result<BTreeMap<PathBuf, TreeEntry>> {
    let mut tree = BTreeMap::<PathBuf, TreeEntry>::new();
    for file in rendered {
        if matches!(file.strategy, FileStrategy::Skip) {
            continue;
        }
        let existing = tree.remove(&file.destination);
        let entry = match (&file.strategy, existing) {
            (FileStrategy::Skip, _) => unreachable!("skip files are filtered above"),
            (FileStrategy::Create, Some(current)) => {
                if normalize_eol(&current.contents) != normalize_eol(&file.contents) {
                    return Err(ScafflareError::FileConflict {
                        path: file.destination.clone(),
                        reason: format!(
                            "recipe `{}` uses create but another recipe already rendered different content",
                            file.recipe
                        ),
                    });
                }
                with_owner(current, &file.recipe)
            }
            (FileStrategy::Fail, Some(_)) => {
                return Err(ScafflareError::FileConflict {
                    path: file.destination.clone(),
                    reason: format!("recipe `{}` requires this path not to exist", file.recipe),
                });
            }
            (FileStrategy::MergeJson, current) => {
                let left = match &current {
                    Some(entry) => parse_json(&file.destination, &entry.contents)?,
                    None => Value::Object(Default::default()),
                };
                let right = parse_json(&file.destination, &file.contents)?;
                let contents = serialize_json(&deep_merge(left, right))?;
                let mut entry = current.unwrap_or_else(|| TreeEntry {
                    contents: Vec::new(),
                    owners: Vec::new(),
                    strategy: FileStrategy::MergeJson,
                    json: true,
                });
                entry.contents = contents;
                entry.strategy = FileStrategy::MergeJson;
                entry.json = true;
                with_owner(entry, &file.recipe)
            }
            (strategy, current) => {
                let owners = current.map(|entry| entry.owners).unwrap_or_default();
                with_owner(
                    TreeEntry {
                        contents: file.contents.clone(),
                        owners,
                        strategy: strategy.clone(),
                        json: false,
                    },
                    &file.recipe,
                )
            }
        };
        tree.insert(file.destination.clone(), entry);
    }
    Ok(tree)
}

fn with_owner(mut entry: TreeEntry, recipe: &str) -> TreeEntry {
    if !entry.owners.iter().any(|owner| owner == recipe) {
        entry.owners.push(recipe.to_owned());
        entry.owners.sort();
    }
    entry
}

/// Pretty JSON with a trailing newline, the canonical on-disk form.
pub fn serialize_json(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| ScafflareError::Serialization(error.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn parse_json(path: &Path, bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice(bytes).map_err(|error| ScafflareError::FileConflict {
        path: path.to_path_buf(),
        reason: format!("valid JSON is required for merge_json: {error}"),
    })
}

/// Unified diff of a planned write against the file currently on disk.
/// Returns `None` for binary content or when nothing differs.
pub fn unified_diff(root: &Path, change: &PlannedChange) -> Option<String> {
    let new = std::str::from_utf8(change.contents.as_ref()?).ok()?;
    let old_bytes = std::fs::read(root.join(&change.path)).unwrap_or_default();
    let old = std::str::from_utf8(&old_bytes).ok()?;
    if old == new {
        return None;
    }
    let name = change.path.to_string_lossy().replace('\\', "/");
    let mut options = diffy::DiffOptions::new();
    options.set_original_filename(format!("a/{name}"));
    options.set_modified_filename(format!("b/{name}"));
    Some(options.create_patch(old, new).to_string())
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

    fn rendered(recipe: &str, dest: &str, strategy: FileStrategy, body: &str) -> RenderedFile {
        RenderedFile {
            recipe: recipe.to_owned(),
            destination: PathBuf::from(dest),
            strategy,
            contents: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn identical_create_from_two_recipes_has_two_owners() {
        let tree = render_tree(&[
            rendered("a", "x.txt", FileStrategy::Create, "same"),
            rendered("b", "x.txt", FileStrategy::Create, "same"),
        ])
        .unwrap();
        assert_eq!(tree[Path::new("x.txt")].owners, vec!["a", "b"]);
    }

    #[test]
    fn differing_create_from_two_recipes_conflicts() {
        let result = render_tree(&[
            rendered("a", "x.txt", FileStrategy::Create, "one"),
            rendered("b", "x.txt", FileStrategy::Create, "two"),
        ]);
        assert!(matches!(result, Err(ScafflareError::FileConflict { .. })));
    }

    #[test]
    fn merge_json_fragments_compose_without_touching_disk() {
        let tree = render_tree(&[
            rendered(
                "a",
                "package.json",
                FileStrategy::MergeJson,
                r#"{"scripts":{"a":"1"}}"#,
            ),
            rendered(
                "b",
                "package.json",
                FileStrategy::MergeJson,
                r#"{"scripts":{"b":"2"}}"#,
            ),
        ])
        .unwrap();
        let entry = &tree[Path::new("package.json")];
        assert!(entry.json);
        let value: Value = serde_json::from_slice(&entry.contents).unwrap();
        assert_eq!(value, serde_json::json!({"scripts":{"a":"1","b":"2"}}));
        assert!(entry.contents.ends_with(b"\n"));
    }

    #[test]
    fn skip_is_ignored_and_fail_rejects_second_writer() {
        let tree = render_tree(&[rendered("a", "s.txt", FileStrategy::Skip, "x")]).unwrap();
        assert!(tree.is_empty());
        let result = render_tree(&[
            rendered("a", "f.txt", FileStrategy::Create, "x"),
            rendered("b", "f.txt", FileStrategy::Fail, "x"),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn replace_overrides_earlier_content() {
        let tree = render_tree(&[
            rendered("a", "r.txt", FileStrategy::Create, "old"),
            rendered("b", "r.txt", FileStrategy::Replace, "new"),
        ])
        .unwrap();
        assert_eq!(tree[Path::new("r.txt")].contents, b"new");
        assert_eq!(tree[Path::new("r.txt")].owners, vec!["a", "b"]);
    }

    #[test]
    fn invalid_json_fragment_is_a_conflict_not_a_panic() {
        let result = render_tree(&[rendered("a", "p.json", FileStrategy::MergeJson, "{nope")]);
        assert!(matches!(result, Err(ScafflareError::FileConflict { .. })));
    }
}
