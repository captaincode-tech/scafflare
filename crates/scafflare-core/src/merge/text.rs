//! Line-based three-way text merge (diff3) on top of `diffy`.
//!
//! All inputs must already be end-of-line normalized (LF). Use [`uses_crlf`] and
//! [`restore_eol`] to preserve the working tree's line-ending style.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextMerge {
    Clean(String),
    /// Merged text containing git-style conflict markers.
    Conflict(String),
}

pub fn merge3(base: &str, ours: &str, theirs: &str) -> TextMerge {
    if ours == theirs {
        return TextMerge::Clean(ours.to_owned());
    }
    if base == ours {
        return TextMerge::Clean(theirs.to_owned());
    }
    if base == theirs {
        return TextMerge::Clean(ours.to_owned());
    }
    match diffy::merge(base, ours, theirs) {
        Ok(merged) => TextMerge::Clean(merged),
        Err(conflicted) => TextMerge::Conflict(conflicted),
    }
}

pub fn uses_crlf(bytes: &[u8]) -> bool {
    bytes.windows(2).any(|pair| pair == b"\r\n")
}

/// Convert LF text back to CRLF when the working tree uses CRLF.
pub fn restore_eol(text: &str, crlf: bool) -> Vec<u8> {
    if crlf {
        text.replace('\n', "\r\n").into_bytes()
    } else {
        text.as_bytes().to_vec()
    }
}

/// True when `text` contains a complete conflict-marker block.
pub fn has_conflict_markers(text: &str) -> bool {
    let mut open = false;
    let mut middle = false;
    for line in text.lines() {
        if line.starts_with("<<<<<<< ") || line == "<<<<<<<" {
            open = true;
            middle = false;
        } else if open && line == "=======" {
            middle = true;
        } else if open && middle && (line.starts_with(">>>>>>> ") || line == ">>>>>>>") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(base: &str, ours: &str, theirs: &str) -> String {
        match merge3(base, ours, theirs) {
            TextMerge::Clean(text) => text,
            TextMerge::Conflict(text) => panic!("unexpected conflict:\n{text}"),
        }
    }

    const BASE: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n";

    #[test]
    fn identical_sides_and_unchanged_sides() {
        assert_eq!(clean(BASE, BASE, BASE), BASE);
        assert_eq!(clean(BASE, "x\n", "x\n"), "x\n");
        assert_eq!(clean(BASE, BASE, "t\n"), "t\n");
        assert_eq!(clean(BASE, "o\n", BASE), "o\n");
    }

    #[test]
    fn non_overlapping_edits_merge() {
        let ours = BASE.replace("two", "TWO");
        let theirs = BASE.replace("seven", "SEVEN");
        assert_eq!(
            clean(BASE, &ours, &theirs),
            "one\nTWO\nthree\nfour\nfive\nsix\nSEVEN\neight\n"
        );
    }

    #[test]
    fn same_line_conflict_produces_markers() {
        let ours = BASE.replace("four", "ours");
        let theirs = BASE.replace("four", "theirs");
        match merge3(BASE, &ours, &theirs) {
            TextMerge::Conflict(text) => {
                assert!(has_conflict_markers(&text));
                assert!(text.contains("ours") && text.contains("theirs"));
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn identical_change_on_both_sides_is_not_a_conflict() {
        // Crash-recovery shape: disk already contains the template change plus a user edit.
        let theirs = BASE.replace("seven", "SEVEN");
        let ours = theirs.replace("two", "TWO");
        assert_eq!(clean(BASE, &ours, &theirs), ours);
    }

    #[test]
    fn handles_missing_trailing_newline_empty_and_rewrites() {
        // Without a trailing newline the last line differs from "b\n", so the
        // appended line overlaps the edited one: a genuine conflict.
        assert!(matches!(
            merge3("a\nb", "a\nB", "a\nb\nc"),
            TextMerge::Conflict(_)
        ));
        assert_eq!(
            clean("a\nb\nm\n", "A\nb\nm\n", "a\nb\nm\nc\n"),
            "A\nb\nm\nc\n"
        );
        assert_eq!(clean("", "x\n", "x\n"), "x\n");
        assert_eq!(clean("", "", "x\n"), "x\n");
        assert!(matches!(
            merge3("a\nb\nc\n", "1\n2\n3\n", "x\ny\nz\n"),
            TextMerge::Conflict(_)
        ));
    }

    #[test]
    fn handles_unicode_and_persian_text() {
        let base = "سلام\nدنیا\nمتن\n";
        let ours = "سلام!\nدنیا\nمتن\n";
        let theirs = "سلام\nدنیا\nمتن 🚀\n";
        assert_eq!(clean(base, ours, theirs), "سلام!\nدنیا\nمتن 🚀\n");
    }

    #[test]
    fn preserves_crlf_style() {
        assert!(uses_crlf(b"a\r\nb"));
        assert!(!uses_crlf(b"a\nb"));
        assert_eq!(restore_eol("a\nb\n", true), b"a\r\nb\r\n");
        assert_eq!(restore_eol("a\nb\n", false), b"a\nb\n");
    }

    #[test]
    fn detects_marker_blocks_only_when_complete() {
        assert!(has_conflict_markers(
            "a\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n"
        ));
        assert!(!has_conflict_markers("a\n=======\nb\n"));
        assert!(!has_conflict_markers("<<<<<<< ours\nonly open\n"));
    }

    mod properties {
        use proptest::prelude::*;

        use super::super::*;

        proptest! {
            #[test]
            fn trivial_sides_follow_the_other_side(base in ".{0,60}", other in ".{0,60}") {
                prop_assert_eq!(merge3(&base, &base, &other), TextMerge::Clean(other.clone()));
                prop_assert_eq!(merge3(&base, &other, &base), TextMerge::Clean(other.clone()));
                prop_assert_eq!(merge3(&base, &other, &other), TextMerge::Clean(other));
            }

            #[test]
            fn never_panics_on_arbitrary_text(a in ".{0,80}", b in ".{0,80}", c in ".{0,80}") {
                let first = merge3(&a, &b, &c);
                prop_assert_eq!(first, merge3(&a, &b, &c));
            }

            #[test]
            fn disjoint_edits_apply_both_without_conflict(
                lines in 6usize..30,
                choices in proptest::collection::vec(0u8..3, 10),
            ) {
                let base: Vec<String> = (0..lines).map(|i| format!("line {i}")).collect();
                let mut ours = base.clone();
                let mut theirs = base.clone();
                let mut expected = base.clone();
                for (slot, choice) in choices.iter().enumerate() {
                    let index = slot * 3;
                    if index >= lines { break; }
                    match choice {
                        1 => { ours[index] = format!("ours {index}"); expected[index] = ours[index].clone(); }
                        2 => { theirs[index] = format!("theirs {index}"); expected[index] = theirs[index].clone(); }
                        _ => {}
                    }
                }
                let join = |v: &Vec<String>| v.join("\n") + "\n";
                prop_assert_eq!(
                    merge3(&join(&base), &join(&ours), &join(&theirs)),
                    TextMerge::Clean(join(&expected))
                );
            }

            #[test]
            fn crlf_roundtrip_is_lossless(text in "[a-z\\n]{0,40}") {
                prop_assert_eq!(restore_eol(&text, true).len(), text.len() + text.matches('\n').count());
                prop_assert_eq!(restore_eol(&text, false), text.as_bytes().to_vec());
            }
        }
    }
}
