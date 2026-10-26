use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::error::{Result, ScafflareError};
use crate::plan::{ChangeKind, FilePlan, PlannedChange};

#[derive(Debug, Clone)]
struct AppliedChange {
    path: PathBuf,
    backup: Option<PathBuf>,
}

/// Application order: project files, new base blobs, the lockfile, and finally
/// garbage collection of obsolete blobs.
///
/// Writing the lockfile after the files makes an interrupted run self-healing: a
/// rerun sees written files as identical to the desired render and converges.
/// Old blobs are deleted only after the lockfile stops referencing them, so a
/// crash can never leave the previous lockfile pointing at a missing ancestor.
fn apply_rank(change: &PlannedChange) -> u8 {
    if change.path == Path::new(".scafflare/lock.yaml") {
        2
    } else if crate::blobs::is_blob_path(&change.path) {
        if change.kind == ChangeKind::Delete {
            3
        } else {
            1
        }
    } else {
        0
    }
}

/// The changes of a plan that touch the disk, in application order.
pub fn ordered_changes(plan: &FilePlan) -> Vec<&PlannedChange> {
    let mut changes: Vec<&PlannedChange> = plan
        .changes
        .iter()
        .filter(|change| {
            matches!(
                change.kind,
                ChangeKind::Create | ChangeKind::Update | ChangeKind::Delete
            )
        })
        .collect();
    changes.sort_by_key(|change| apply_rank(change));
    changes
}

pub fn apply_plan(root: &Path, plan: &FilePlan) -> Result<()> {
    apply_plan_limited(root, plan, None)
}

/// Apply at most `limit` changes and stop without rollback, simulating a crash.
/// Intended for crash-convergence tests; production code passes `None`.
#[doc(hidden)]
pub fn apply_plan_limited(root: &Path, plan: &FilePlan, limit: Option<usize>) -> Result<()> {
    let mut changes = ordered_changes(plan);
    if let Some(limit) = limit {
        changes.truncate(limit);
    }
    if changes.is_empty() {
        return Ok(());
    }

    fs::create_dir_all(root).map_err(|error| ScafflareError::io(root, error))?;
    for change in &changes {
        ensure_safe_target(root, &change.path)?;
    }
    let transaction_root = root
        .join(".scafflare")
        .join("transactions")
        .join(unique_id());
    let stage_root = transaction_root.join("stage");
    let backup_root = transaction_root.join("backup");
    fs::create_dir_all(&stage_root).map_err(|error| ScafflareError::io(&stage_root, error))?;
    fs::create_dir_all(&backup_root).map_err(|error| ScafflareError::io(&backup_root, error))?;

    for change in &changes {
        if matches!(change.kind, ChangeKind::Create | ChangeKind::Update) {
            let staged_path = stage_root.join(&change.path);
            let parent = staged_path
                .parent()
                .expect("relative path must have a parent");
            fs::create_dir_all(parent).map_err(|error| ScafflareError::io(parent, error))?;
            let contents = change
                .contents
                .as_ref()
                .ok_or_else(|| ScafflareError::Transaction {
                    path: change.path.clone(),
                    reason: "planned write is missing staged contents".to_owned(),
                })?;
            fs::write(&staged_path, contents)
                .map_err(|error| ScafflareError::io(&staged_path, error))?;
        }
    }

    let mut applied = Vec::new();
    for change in changes {
        if let Err(error) = apply_change(root, &stage_root, &backup_root, change, &mut applied) {
            let rollback_result = rollback(root, &applied);
            let _ = fs::remove_dir_all(&transaction_root);
            return match rollback_result {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(ScafflareError::Transaction {
                    path: root.to_path_buf(),
                    reason: format!("{error}; rollback also failed: {rollback_error}"),
                }),
            };
        }
    }
    fs::remove_dir_all(&transaction_root)
        .map_err(|error| ScafflareError::io(&transaction_root, error))?;
    prune_empty_transaction_parent(root);
    Ok(())
}

fn apply_change(
    root: &Path,
    stage_root: &Path,
    backup_root: &Path,
    change: &PlannedChange,
    applied: &mut Vec<AppliedChange>,
) -> Result<()> {
    let target = root.join(&change.path);
    let backup = if target.exists() {
        let backup = backup_root.join(&change.path);
        let parent = backup.parent().expect("relative path must have a parent");
        fs::create_dir_all(parent).map_err(|error| ScafflareError::io(parent, error))?;
        fs::copy(&target, &backup).map_err(|error| ScafflareError::io(&target, error))?;
        Some(backup)
    } else {
        None
    };

    match change.kind {
        ChangeKind::Create | ChangeKind::Update => {
            let staged = stage_root.join(&change.path);
            let parent = target.parent().expect("relative path must have a parent");
            fs::create_dir_all(parent).map_err(|error| ScafflareError::io(parent, error))?;
            if target.exists() {
                fs::remove_file(&target).map_err(|error| ScafflareError::io(&target, error))?;
            }
            fs::rename(&staged, &target).map_err(|error| ScafflareError::Transaction {
                path: target.clone(),
                reason: error.to_string(),
            })?;
        }
        ChangeKind::Delete => {
            if target.exists() {
                fs::remove_file(&target).map_err(|error| ScafflareError::io(&target, error))?;
            }
        }
        ChangeKind::Unchanged | ChangeKind::Skip => return Ok(()),
    }
    applied.push(AppliedChange {
        path: change.path.clone(),
        backup,
    });
    Ok(())
}

fn ensure_safe_target(root: &Path, relative: &Path) -> Result<()> {
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(ScafflareError::Transaction {
            path: relative.to_path_buf(),
            reason: "transaction path must be relative".to_owned(),
        });
    }
    let mut current = root.to_path_buf();
    for part in relative.components() {
        if let std::path::Component::Normal(name) = part {
            current.push(name);
            if let Ok(metadata) = fs::symlink_metadata(&current) {
                if metadata.file_type().is_symlink() {
                    return Err(ScafflareError::Transaction {
                        path: relative.to_path_buf(),
                        reason: "refusing symbolic-link path component".to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn rollback(root: &Path, applied: &[AppliedChange]) -> Result<()> {
    for change in applied.iter().rev() {
        let target = root.join(&change.path);
        if target.exists() {
            fs::remove_file(&target).map_err(|error| ScafflareError::io(&target, error))?;
        }
        if let Some(backup) = &change.backup {
            let parent = target.parent().expect("relative path must have a parent");
            fs::create_dir_all(parent).map_err(|error| ScafflareError::io(parent, error))?;
            fs::copy(backup, &target).map_err(|error| ScafflareError::io(&target, error))?;
        }
    }
    Ok(())
}

pub fn sha256_hex(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

fn unique_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

fn prune_empty_transaction_parent(root: &Path) {
    let transactions = root.join(".scafflare").join("transactions");
    if transactions
        .read_dir()
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
    {
        let _ = fs::remove_dir(&transactions);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::plan::{ChangeKind, FilePlan, PlannedChange};

    fn change(kind: ChangeKind, path: &str) -> PlannedChange {
        PlannedChange {
            kind,
            path: PathBuf::from(path),
            recipe: "test".to_owned(),
            contents: Some(b"x\n".to_vec()),
        }
    }

    #[test]
    fn orders_files_then_blobs_then_lockfile_last() {
        let hash = "a".repeat(64);
        let blob = format!(".scafflare/base/{hash}");
        let plan = FilePlan {
            changes: vec![
                change(ChangeKind::Create, ".scafflare/lock.yaml"),
                change(ChangeKind::Create, &blob),
                change(ChangeKind::Create, "src/a.ts"),
                change(ChangeKind::Unchanged, "skipped.ts"),
                change(ChangeKind::Delete, "old.ts"),
                change(
                    ChangeKind::Delete,
                    &format!(".scafflare/base/{}", "b".repeat(64)),
                ),
            ],
        };
        let order: Vec<String> = ordered_changes(&plan)
            .iter()
            .map(|change| change.path.to_string_lossy().replace('\\', "/"))
            .collect();
        let old_blob = format!(".scafflare/base/{}", "b".repeat(64));
        assert_eq!(
            order,
            vec![
                "src/a.ts",
                "old.ts",
                &blob,
                ".scafflare/lock.yaml",
                &old_blob
            ]
        );
    }

    #[test]
    fn limited_apply_stops_before_lockfile() {
        let temporary = tempfile::tempdir().unwrap();
        let plan = FilePlan {
            changes: vec![
                change(ChangeKind::Create, ".scafflare/lock.yaml"),
                change(ChangeKind::Create, "src/a.ts"),
            ],
        };
        apply_plan_limited(temporary.path(), &plan, Some(1)).unwrap();
        assert!(temporary.path().join("src/a.ts").is_file());
        assert!(!temporary.path().join(".scafflare/lock.yaml").exists());
        assert!(!temporary.path().join(".scafflare/transactions").exists());
    }

    #[test]
    fn applies_and_hashes_content() {
        let temporary = tempfile::tempdir().unwrap();
        let plan = FilePlan {
            changes: vec![PlannedChange {
                kind: ChangeKind::Create,
                path: PathBuf::from("src/main.ts"),
                recipe: "test".to_owned(),
                contents: Some(b"export {};\n".to_vec()),
            }],
        };
        apply_plan(temporary.path(), &plan).unwrap();
        assert_eq!(
            fs::read_to_string(temporary.path().join("src/main.ts")).unwrap(),
            "export {};\n"
        );
        assert_eq!(
            sha256_hex(b"x"),
            "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881"
        );
    }
}
