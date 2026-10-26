mod common;

use std::fs;
use std::path::PathBuf;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/golden/trees")
        .join(format!("{name}.txt"))
}

#[test]
fn init_output_matches_golden_trees() {
    for (name, sets) in common::combos() {
        let temporary = tempfile::tempdir().unwrap();
        common::init_combo(temporary.path(), name, &sets);
        let actual = common::tree_listing(&temporary.path().join(name));
        let path = golden_path(name);
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            fs::write(&path, &actual).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing golden {}", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(actual, expected, "golden tree drifted for {name}");
    }
}
