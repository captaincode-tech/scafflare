//! Three-way reconciliation of rendered recipes against a working tree.
//!
//! For every managed path three versions exist: the *base* (the pure render
//! recorded at the last Scafflare operation), *ours* (what is on disk now) and
//! *theirs* (the pure render of the current recipes). The engine never writes a
//! file until the whole project has been analysed, so a conflict in abort mode
//! leaves the tree byte-identical.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::blobs::{self, content_hash, normalize_eol, read_blob};
use crate::error::{Result, ScafflareError};
use crate::merge::json::{self as json_merge, JsonMerge};
use crate::merge::text::{self, TextMerge};
use crate::plan::{
    deep_merge, parse_json, render_tree, serialize_json, ChangeKind, FilePlan, PlannedChange,
    TreeEntry,
};
use crate::recipe::FileStrategy;
use crate::render::RenderedFile;
use crate::state::{lockfile_path, serialize_state, ManagedFile, ProjectState};

pub const SIDECAR_SUFFIX: &str = ".scafflare-new";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Init,
    Add,
    Upgrade,
    /// Conservative: never removes dependencies and preserves modified files.
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictMode {
    /// Write nothing and report every conflict.
    #[default]
    Abort,
    /// Write git-style markers (text) or `<path>.scafflare-new` sidecars.
    Markers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    BothModified,
    NoBase,
    UnmanagedExists,
    DeletedByUser,
    InvalidJson,
}

#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub path: PathBuf,
    pub kind: ConflictKind,
    pub recipe: String,
    pub json_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Created,
    /// Template changed, user did not: fast-forwarded.
    Updated,
    /// Both sides changed and merged cleanly.
    Merged,
    /// Template did not change; user edits kept.
    Kept,
    Unchanged,
    /// Existing identical file now managed.
    Adopted,
    Deleted,
    /// No longer rendered but modified: file kept, no longer managed.
    Orphaned,
    /// User deleted the file and the template did not change.
    DeletionHonored,
    /// Remove mode: modified file left alone.
    Preserved,
    Conflicted,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub path: PathBuf,
    pub kind: OutcomeKind,
    pub recipe: String,
}

#[derive(Debug, Clone)]
pub struct Reconciled {
    pub plan: FilePlan,
    pub state: ProjectState,
    pub outcomes: Vec<Outcome>,
    pub conflicts: Vec<Conflict>,
}

pub struct ReconcileInput<'a> {
    pub root: &'a Path,
    pub rendered: &'a [RenderedFile],
    /// Managed files recorded before this operation.
    pub old_managed: &'a BTreeMap<String, ManagedFile>,
    /// State to persist (recipes and variables already updated). `managed_files` is replaced.
    pub next_state: ProjectState,
    pub mode: Mode,
    pub conflict_mode: ConflictMode,
}

enum Action {
    /// Leave the file alone.
    Keep,
    Write(Vec<u8>),
    Delete,
    /// Record a skipped change (preserved or orphaned file).
    Skip,
}

struct Decision {
    action: Action,
    outcome: OutcomeKind,
    conflict: Option<(ConflictKind, Vec<String>)>,
    /// Sidecar contents to write when conflicts are materialized.
    sidecar: bool,
    /// Whether the recorded base should advance to the current render.
    advance_base: bool,
    /// Whether the path stays in the managed map.
    managed: bool,
}

impl Decision {
    fn new(action: Action, outcome: OutcomeKind) -> Self {
        Self {
            action,
            outcome,
            conflict: None,
            sidecar: false,
            advance_base: true,
            managed: true,
        }
    }

    fn conflict(kind: ConflictKind, json_paths: Vec<String>) -> Self {
        Self {
            action: Action::Keep,
            outcome: OutcomeKind::Conflicted,
            conflict: Some((kind, json_paths)),
            sidecar: true,
            advance_base: true,
            managed: true,
        }
    }
}

pub fn sidecar_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(SIDECAR_SUFFIX);
    PathBuf::from(name)
}

fn read_optional(root: &Path, relative: &Path) -> Result<Option<Vec<u8>>> {
    let path = root.join(relative);
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ScafflareError::io(&path, error)),
    }
}

fn same(json: bool, left: &[u8], right: &[u8]) -> bool {
    let (left, right) = (normalize_eol(left), normalize_eol(right));
    if left == right {
        return true;
    }
    if json {
        if let (Ok(a), Ok(b)) = (
            serde_json::from_slice::<Value>(&left),
            serde_json::from_slice::<Value>(&right),
        ) {
            return a == b;
        }
    }
    false
}

/// Restyle freshly produced LF content to the working tree's line endings.
fn in_style_of(existing: Option<&[u8]>, contents: &[u8]) -> Vec<u8> {
    match existing {
        Some(existing) if text::uses_crlf(existing) => {
            let normalized = normalize_eol(contents);
            String::from_utf8(normalized.clone())
                .map(|text| text::restore_eol(&text, true))
                .unwrap_or(normalized)
        }
        _ => contents.to_vec(),
    }
}

fn pointers(conflicts: Vec<json_merge::JsonConflict>) -> Vec<String> {
    conflicts.into_iter().map(|c| c.pointer).collect()
}

/// `Ok(None)` means the merge result equals the working tree (nothing to write).
type JsonOutcome = std::result::Result<Option<Vec<u8>>, (ConflictKind, Vec<String>)>;

fn merge_json_bytes(
    base: Option<&[u8]>,
    ours: &[u8],
    theirs: &[u8],
    no_base_kind: bool,
) -> Result<JsonOutcome> {
    let Ok(ours_value) = serde_json::from_slice::<Value>(&normalize_eol(ours)) else {
        return Ok(Err((ConflictKind::InvalidJson, Vec::new())));
    };
    let theirs_value: Value = serde_json::from_slice(&normalize_eol(theirs))
        .map_err(|error| ScafflareError::Serialization(error.to_string()))?;
    let base_value =
        base.and_then(|bytes| serde_json::from_slice::<Value>(&normalize_eol(bytes)).ok());
    match json_merge::merge3(base_value.as_ref(), &ours_value, &theirs_value) {
        JsonMerge::Clean(merged) if merged == ours_value => Ok(Ok(None)),
        JsonMerge::Clean(merged) => {
            Ok(Ok(Some(in_style_of(Some(ours), &serialize_json(&merged)?))))
        }
        JsonMerge::Conflicts(conflicts) => {
            let kind = if no_base_kind {
                ConflictKind::NoBase
            } else {
                ConflictKind::BothModified
            };
            Ok(Err((kind, pointers(conflicts))))
        }
    }
}

fn merge_text_bytes(base: &[u8], ours: &[u8], theirs: &[u8], markers: bool) -> Decision {
    let (Ok(base), Ok(ours_text), Ok(theirs)) = (
        String::from_utf8(normalize_eol(base)),
        String::from_utf8(normalize_eol(ours)),
        String::from_utf8(normalize_eol(theirs)),
    ) else {
        return Decision::conflict(ConflictKind::BothModified, Vec::new());
    };
    match text::merge3(&base, &ours_text, &theirs) {
        TextMerge::Clean(merged) => {
            let bytes = text::restore_eol(&merged, text::uses_crlf(ours));
            Decision::new(Action::Write(bytes), OutcomeKind::Merged)
        }
        TextMerge::Conflict(marked) => {
            let mut decision = Decision::conflict(ConflictKind::BothModified, Vec::new());
            if markers {
                decision.action = Action::Write(text::restore_eol(&marked, text::uses_crlf(ours)));
                decision.sidecar = false;
            }
            decision
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn decide(
    mode: Mode,
    markers: bool,
    theirs: Option<&TreeEntry>,
    managed: Option<&ManagedFile>,
    base: Option<&[u8]>,
    disk: Option<&[u8]>,
    path: &Path,
) -> Result<Decision> {
    let remove_mode = mode == Mode::Remove;
    match (theirs, managed) {
        // Path is rendered and unknown to the lockfile.
        (Some(entry), None) => {
            let Some(ours) = disk else {
                return Ok(Decision::new(
                    Action::Write(entry.contents.clone()),
                    OutcomeKind::Created,
                ));
            };
            if same(entry.json, ours, &entry.contents) {
                return Ok(Decision::new(Action::Keep, OutcomeKind::Adopted));
            }
            if remove_mode {
                let mut decision = Decision::new(Action::Skip, OutcomeKind::Preserved);
                decision.managed = false;
                return Ok(decision);
            }
            if entry.json {
                return Ok(match merge_json_bytes(None, ours, &entry.contents, true)? {
                    Ok(Some(bytes)) => Decision::new(Action::Write(bytes), OutcomeKind::Merged),
                    Ok(None) => Decision::new(Action::Keep, OutcomeKind::Adopted),
                    Err((_, json_paths)) => {
                        Decision::conflict(ConflictKind::UnmanagedExists, json_paths)
                    }
                });
            }
            if matches!(entry.strategy, FileStrategy::Replace) {
                return Ok(Decision::new(
                    Action::Write(in_style_of(Some(ours), &entry.contents)),
                    OutcomeKind::Updated,
                ));
            }
            Ok(Decision::conflict(
                ConflictKind::UnmanagedExists,
                Vec::new(),
            ))
        }
        // Path is rendered and managed.
        (Some(entry), Some(managed)) => {
            let theirs_bytes = &entry.contents;
            let Some(ours) = disk else {
                if base.is_some_and(|b| same(entry.json, b, theirs_bytes)) {
                    let mut decision = Decision::new(Action::Keep, OutcomeKind::DeletionHonored);
                    decision.advance_base = true;
                    return Ok(decision);
                }
                if remove_mode {
                    let mut decision = Decision::new(Action::Skip, OutcomeKind::Preserved);
                    decision.advance_base = false;
                    return Ok(decision);
                }
                return Ok(Decision::conflict(ConflictKind::DeletedByUser, Vec::new()));
            };
            if same(entry.json, ours, theirs_bytes) {
                return Ok(Decision::new(Action::Keep, OutcomeKind::Unchanged));
            }
            let write_theirs = || Action::Write(in_style_of(Some(ours), theirs_bytes));
            let fast_forward = match base {
                Some(b) => same(entry.json, ours, b),
                None => content_hash(ours) == managed.sha256,
            };
            if remove_mode {
                if entry.json {
                    // Removal never deletes manifest entries: additive merge only.
                    let left = parse_json(path, &normalize_eol(ours));
                    let right = parse_json(path, &normalize_eol(theirs_bytes))?;
                    return Ok(match left {
                        Ok(left) => {
                            let merged = deep_merge(left.clone(), right);
                            if merged == left {
                                Decision::new(Action::Keep, OutcomeKind::Kept)
                            } else {
                                Decision::new(
                                    Action::Write(in_style_of(
                                        Some(ours),
                                        &serialize_json(&merged)?,
                                    )),
                                    OutcomeKind::Merged,
                                )
                            }
                        }
                        Err(_) => {
                            let mut decision = Decision::new(Action::Skip, OutcomeKind::Preserved);
                            decision.advance_base = false;
                            decision
                        }
                    });
                }
                if fast_forward {
                    return Ok(Decision::new(write_theirs(), OutcomeKind::Updated));
                }
                if base.is_some_and(|b| same(entry.json, theirs_bytes, b)) {
                    return Ok(Decision::new(Action::Keep, OutcomeKind::Kept));
                }
                let mut decision = Decision::new(Action::Skip, OutcomeKind::Preserved);
                decision.advance_base = false;
                return Ok(decision);
            }
            match base {
                Some(b) => {
                    if fast_forward {
                        return Ok(Decision::new(write_theirs(), OutcomeKind::Updated));
                    }
                    if same(entry.json, theirs_bytes, b) {
                        return Ok(Decision::new(Action::Keep, OutcomeKind::Kept));
                    }
                    if entry.json {
                        return Ok(
                            match merge_json_bytes(Some(b), ours, theirs_bytes, false)? {
                                Ok(Some(bytes)) => {
                                    Decision::new(Action::Write(bytes), OutcomeKind::Merged)
                                }
                                Ok(None) => Decision::new(Action::Keep, OutcomeKind::Kept),
                                Err((kind, json_paths)) => Decision::conflict(kind, json_paths),
                            },
                        );
                    }
                    Ok(merge_text_bytes(b, ours, theirs_bytes, markers))
                }
                None => {
                    if fast_forward {
                        return Ok(Decision::new(write_theirs(), OutcomeKind::Updated));
                    }
                    if entry.json {
                        return Ok(match merge_json_bytes(None, ours, theirs_bytes, true)? {
                            Ok(Some(bytes)) => {
                                Decision::new(Action::Write(bytes), OutcomeKind::Merged)
                            }
                            Ok(None) => Decision::new(Action::Keep, OutcomeKind::Kept),
                            Err((kind, json_paths)) => Decision::conflict(kind, json_paths),
                        });
                    }
                    Ok(Decision::conflict(ConflictKind::NoBase, Vec::new()))
                }
            }
        }
        // Path is no longer rendered.
        (None, Some(managed)) => {
            let Some(ours) = disk else {
                let mut decision = Decision::new(Action::Keep, OutcomeKind::Deleted);
                decision.managed = false;
                return Ok(decision);
            };
            let unmodified = match base {
                Some(b) => same(false, ours, b),
                None => content_hash(ours) == managed.sha256,
            };
            let mut decision = if unmodified {
                Decision::new(Action::Delete, OutcomeKind::Deleted)
            } else {
                Decision::new(Action::Skip, OutcomeKind::Orphaned)
            };
            decision.managed = false;
            Ok(decision)
        }
        (None, None) => unreachable!("paths come from the union of tree and managed files"),
    }
}

pub fn reconcile(input: ReconcileInput<'_>) -> Result<Reconciled> {
    let ReconcileInput {
        root,
        rendered,
        old_managed,
        mut next_state,
        mode,
        conflict_mode,
    } = input;
    let tree = render_tree(rendered)?;
    let markers = conflict_mode == ConflictMode::Markers;

    let paths: BTreeSet<PathBuf> = tree
        .keys()
        .cloned()
        .chain(old_managed.keys().map(PathBuf::from))
        .collect();

    let mut plan = FilePlan::default();
    let mut outcomes = Vec::new();
    let mut conflicts = Vec::new();
    let mut managed_files = BTreeMap::<String, ManagedFile>::new();
    // Normalized contents of every base the new lockfile will reference.
    let mut needed = BTreeMap::<String, Option<Vec<u8>>>::new();

    for path in paths {
        let key = path.to_string_lossy().replace('\\', "/");
        let entry = tree.get(&path);
        let managed = old_managed.get(&key);
        let base_bytes = managed
            .and_then(|m| m.base.as_deref())
            .and_then(|hash| read_blob(root, hash));
        let disk = read_optional(root, &path)?;
        let decision = decide(
            mode,
            markers,
            entry,
            managed,
            base_bytes.as_deref(),
            disk.as_deref(),
            &path,
        )?;

        let recipe = entry
            .and_then(|e| e.owners.last().cloned())
            .or_else(|| managed.and_then(|m| m.recipes.last().cloned()))
            .unwrap_or_else(|| "unknown".to_owned());

        if let Some((kind, json_paths)) = &decision.conflict {
            conflicts.push(Conflict {
                path: path.clone(),
                kind: *kind,
                recipe: recipe.clone(),
                json_paths: json_paths.clone(),
            });
        }
        outcomes.push(Outcome {
            path: path.clone(),
            kind: decision.outcome,
            recipe: recipe.clone(),
        });

        // Content left on disk after the change set is applied.
        let final_disk: Option<Vec<u8>> = match &decision.action {
            Action::Write(bytes) => Some(bytes.clone()),
            Action::Delete => None,
            Action::Keep | Action::Skip => disk.clone(),
        };

        let conflicted_in_abort = decision.conflict.is_some() && !markers;
        if !conflicted_in_abort {
            match &decision.action {
                Action::Write(bytes) => {
                    let kind = if disk.is_some() {
                        ChangeKind::Update
                    } else {
                        ChangeKind::Create
                    };
                    plan.changes.push(PlannedChange {
                        kind,
                        path: path.clone(),
                        recipe: recipe.clone(),
                        contents: Some(bytes.clone()),
                    });
                }
                Action::Delete => plan.changes.push(PlannedChange {
                    kind: ChangeKind::Delete,
                    path: path.clone(),
                    recipe: recipe.clone(),
                    contents: None,
                }),
                Action::Skip => plan.changes.push(PlannedChange {
                    kind: ChangeKind::Skip,
                    path: path.clone(),
                    recipe: recipe.clone(),
                    contents: None,
                }),
                Action::Keep => {}
            }
            if decision.conflict.is_some() && decision.sidecar {
                if let Some(entry) = entry {
                    let sidecar = sidecar_path(&path);
                    let exists = root.join(&sidecar).exists();
                    plan.changes.push(PlannedChange {
                        kind: if exists {
                            ChangeKind::Update
                        } else {
                            ChangeKind::Create
                        },
                        path: sidecar,
                        recipe: recipe.clone(),
                        contents: Some(entry.contents.clone()),
                    });
                }
            }
        }

        if !decision.managed {
            continue;
        }
        let Some(entry) = entry else { continue };
        let (base_hash, sha256) = if decision.advance_base {
            let normalized = normalize_eol(&entry.contents);
            let hash = content_hash(&normalized);
            needed.insert(hash.clone(), Some(normalized));
            let sha = match (&final_disk, managed) {
                (Some(bytes), _) => content_hash(bytes),
                (None, Some(m)) => m.sha256.clone(),
                (None, None) => hash.clone(),
            };
            (Some(hash), sha)
        } else {
            let old = managed
                .and_then(|m| m.base.clone())
                .filter(|_| base_bytes.is_some());
            if let (Some(hash), Some(bytes)) = (&old, &base_bytes) {
                needed
                    .entry(hash.clone())
                    .or_insert_with(|| Some(bytes.clone()));
            }
            let sha = final_disk
                .as_deref()
                .map(content_hash)
                .or_else(|| managed.map(|m| m.sha256.clone()))
                .unwrap_or_default();
            (old, sha)
        };
        managed_files.insert(
            key,
            ManagedFile {
                recipes: entry.owners.clone(),
                sha256,
                base: base_hash,
            },
        );
    }

    if !conflicts.is_empty() && !markers {
        // Abort mode: report only; never touch the tree.
        next_state.managed_files = old_managed.clone();
        return Ok(Reconciled {
            plan: FilePlan::default(),
            state: next_state,
            outcomes,
            conflicts,
        });
    }

    next_state.managed_files = managed_files;
    append_blob_changes(root, &needed, &mut plan)?;
    append_lockfile_change(root, &mut plan, &next_state)?;
    plan.changes.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.recipe.cmp(&right.recipe))
    });
    Ok(Reconciled {
        plan,
        state: next_state,
        outcomes,
        conflicts,
    })
}

/// Create missing base blobs and garbage-collect unreferenced ones.
fn append_blob_changes(
    root: &Path,
    needed: &BTreeMap<String, Option<Vec<u8>>>,
    plan: &mut FilePlan,
) -> Result<()> {
    let existing = blobs::list_blobs(root)?;
    for (hash, contents) in needed {
        let valid = existing.contains(hash) && read_blob(root, hash).is_some();
        if valid {
            continue;
        }
        let Some(contents) = contents else { continue };
        plan.changes.push(PlannedChange {
            kind: if existing.contains(hash) {
                ChangeKind::Update
            } else {
                ChangeKind::Create
            },
            path: blobs::blob_relative(hash),
            recipe: "scafflare-state".to_owned(),
            contents: Some(contents.clone()),
        });
    }
    for name in existing {
        if blobs::is_valid_hash(&name) && !needed.contains_key(&name) {
            plan.changes.push(PlannedChange {
                kind: ChangeKind::Delete,
                path: blobs::blob_relative(&name),
                recipe: "scafflare-state".to_owned(),
                contents: None,
            });
        }
    }
    Ok(())
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
    Ok(())
}

/// Managed paths that still carry conflict markers or `.scafflare-new` sidecars.
pub fn unresolved_conflicts(root: &Path, managed: &BTreeMap<String, ManagedFile>) -> Vec<String> {
    let mut found = Vec::new();
    for (key, file) in managed {
        let relative = Path::new(key);
        if root.join(sidecar_path(relative)).exists() {
            found.push(key.clone());
            continue;
        }
        let Ok(bytes) = fs::read(root.join(relative)) else {
            continue;
        };
        let Ok(text) = String::from_utf8(normalize_eol(&bytes)) else {
            continue;
        };
        if !text::has_conflict_markers(&text) {
            continue;
        }
        let base_has_markers = file
            .base
            .as_deref()
            .and_then(|hash| read_blob(root, hash))
            .and_then(|b| String::from_utf8(b).ok())
            .is_some_and(|b| text::has_conflict_markers(&b));
        if !base_has_markers {
            found.push(key.clone());
        }
    }
    found
}
