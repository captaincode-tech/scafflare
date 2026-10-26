//! Guard: an official recipe's content may not change without a version bump.
//!
//! Upgrades are driven by recipe versions, so shipping new template content under
//! an unchanged version would make `scafflare upgrade` silently skip it.
//!
//! After intentionally changing a recipe, bump `metadata.version` and regenerate
//! the manifest with `UPDATE_CHECKSUMS=1 cargo test -p scafflare-core --test recipe_checksums`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use scafflare_core::blobs::{content_hash, normalize_eol};
use scafflare_core::registry::{FilesystemRegistry, RecipeRegistry};
use sha2::{Digest, Sha256};

fn official_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes/official")
}

fn collect(root: &Path, directory: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let relative = path.strip_prefix(root).unwrap();
            out.push((
                relative.to_string_lossy().replace('\\', "/"),
                normalize_eol(&fs::read(&path).unwrap()),
            ));
        }
    }
}

fn recipe_digest(recipe_dir: &Path) -> String {
    let mut files = Vec::new();
    collect(recipe_dir, recipe_dir, &mut files);
    files.sort();
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(content_hash(&bytes).as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn current() -> BTreeMap<String, (String, String)> {
    let registry = FilesystemRegistry::new(official_root());
    registry
        .list()
        .unwrap()
        .into_iter()
        .map(|name| {
            let version = registry
                .get(&name)
                .unwrap()
                .document
                .metadata
                .version
                .to_string();
            let digest = recipe_digest(&official_root().join(&name));
            (name, (version, digest))
        })
        .collect()
}

fn parse_manifest(text: &str) -> BTreeMap<String, (String, String)> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next().unwrap().to_owned();
            let version = parts.next().unwrap().to_owned();
            let digest = parts.next().unwrap().to_owned();
            (name, (version, digest))
        })
        .collect()
}

#[test]
fn official_recipe_content_changes_require_a_version_bump() {
    let manifest_path = official_root().join("CHECKSUMS");
    let now = current();
    let recorded = fs::read_to_string(&manifest_path)
        .map(|text| parse_manifest(&text))
        .unwrap_or_default();

    if std::env::var_os("UPDATE_CHECKSUMS").is_some() {
        for (name, (version, digest)) in &now {
            if let Some((old_version, old_digest)) = recorded.get(name) {
                assert!(
                    !(old_version == version && old_digest != digest),
                    "recipe `{name}` changed but is still version {version}; bump metadata.version first"
                );
            }
        }
        let mut output = String::from(
            "# name version sha256(content). Regenerate with UPDATE_CHECKSUMS=1 after bumping a recipe version.\n",
        );
        for (name, (version, digest)) in &now {
            output.push_str(&format!("{name} {version} {digest}\n"));
        }
        fs::write(&manifest_path, output).unwrap();
        return;
    }

    for (name, (version, digest)) in &now {
        let (old_version, old_digest) = recorded.get(name).unwrap_or_else(|| {
            panic!("recipe `{name}` missing from recipes/official/CHECKSUMS; run with UPDATE_CHECKSUMS=1")
        });
        if old_version == version {
            assert_eq!(
                old_digest, digest,
                "recipe `{name}` content changed without a version bump (still {version})"
            );
        } else {
            panic!(
                "recipe `{name}` is {version} but CHECKSUMS records {old_version}; run with UPDATE_CHECKSUMS=1"
            );
        }
    }
    for name in recorded.keys() {
        assert!(
            now.contains_key(name),
            "CHECKSUMS lists removed recipe `{name}`"
        );
    }
}
