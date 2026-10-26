//! Content-addressed storage for merge ancestors ("base" renders).
//!
//! Blobs live in `.scafflare/base/<sha256>` without a file extension so that
//! tooling such as tsc, Vitest, and Biome never mistakes them for sources.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{Result, ScafflareError};

pub const BASE_DIR: &str = ".scafflare/base";

/// Convert CRLF line endings to LF so that git `autocrlf` checkouts compare equal.
pub fn normalize_eol(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            index += 1;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    output
}

/// Hash of the end-of-line normalized content. Used for both `sha256` and `base`.
pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(normalize_eol(bytes)))
}

pub fn is_valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn blob_relative(hash: &str) -> PathBuf {
    Path::new(".scafflare").join("base").join(hash)
}

pub fn is_blob_path(path: &Path) -> bool {
    path.starts_with(BASE_DIR) && path.components().count() == 3
}

/// Read a blob and verify its hash. Missing or corrupt blobs yield `None`,
/// which degrades the file to "no base" handling instead of failing.
pub fn read_blob(root: &Path, hash: &str) -> Option<Vec<u8>> {
    if !is_valid_hash(hash) {
        return None;
    }
    let bytes = fs::read(root.join(blob_relative(hash))).ok()?;
    (content_hash(&bytes) == hash).then_some(bytes)
}

/// Names of all blob files currently on disk.
pub fn list_blobs(root: &Path) -> Result<BTreeSet<String>> {
    let directory = root.join(BASE_DIR);
    let mut names = BTreeSet::new();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(names),
        Err(error) => return Err(ScafflareError::io(&directory, error)),
    };
    for entry in entries {
        let entry = entry.map_err(|error| ScafflareError::io(&directory, error))?;
        names.insert(entry.file_name().to_string_lossy().to_string());
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_crlf_but_keeps_lone_cr() {
        assert_eq!(normalize_eol(b"a\r\nb\rc\n"), b"a\nb\rc\n");
        assert_eq!(content_hash(b"a\r\nb"), content_hash(b"a\nb"));
    }

    #[test]
    fn reads_only_verified_blobs() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let hash = content_hash(b"hello\n");
        fs::create_dir_all(root.join(BASE_DIR)).unwrap();
        assert!(read_blob(root, &hash).is_none(), "missing blob");
        fs::write(root.join(blob_relative(&hash)), b"hello\n").unwrap();
        assert_eq!(read_blob(root, &hash).unwrap(), b"hello\n");
        fs::write(root.join(blob_relative(&hash)), b"tampered\n").unwrap();
        assert!(read_blob(root, &hash).is_none(), "corrupt blob");
        assert!(read_blob(root, "../../etc/passwd").is_none());
    }

    #[test]
    fn lists_blobs_and_recognizes_blob_paths() {
        let temporary = tempfile::tempdir().unwrap();
        assert!(list_blobs(temporary.path()).unwrap().is_empty());
        let hash = content_hash(b"x");
        fs::create_dir_all(temporary.path().join(BASE_DIR)).unwrap();
        fs::write(temporary.path().join(blob_relative(&hash)), b"x").unwrap();
        assert!(list_blobs(temporary.path()).unwrap().contains(&hash));
        assert!(is_blob_path(&blob_relative(&hash)));
        assert!(!is_blob_path(Path::new(".scafflare/lock.yaml")));
        assert!(!is_blob_path(Path::new("src/base/x")));
    }
}
