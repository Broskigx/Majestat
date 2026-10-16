//! The numbers the documents claim, checked against the code they claim them about.
//!
//! Every count in the documentation — how many tools the agent has, how many strokes the
//! keyboard answers to, how many kinds of command the log can hold — is a number somebody
//! typed, and a number typed into three files goes stale in one of them. That is not a
//! hypothetical: it has already happened twice in this repository, and both times it was
//! found by reading rather than by running.
//!
//! So the living documents are checked here:
//!
//! * `docs/MCP.md`, `docs/COVERAGE.md` and `README.md` — the tool count, and the number of
//!   command kinds the log can hold.
//! * `docs/UI.md` — the stroke count.
//!
//! Each check asserts that the phrase it looks for was *found* before it compares, because a
//! reworded sentence must fail this test rather than quietly stop being checked.
//!
//! Two counts are deliberately not here. The number of **intentions** and the number of
//! **strings** are lengths of hand-maintained lists inside the interface, so a test derived
//! from them would be comparing two hand-typed numbers rather than a claim against a fact.
//! The `.planning/` documents are not checked either: each of them records the gate that was
//! measured at a moment, in that moment's own numbers, and rewriting them as the code moves
//! would destroy the only record of what was true then.
//!
//! It lives in this crate because this is the layer-6 crate that already holds three of the
//! four numbers — the strokes, the command kinds and, through `maj-mcp`, the tool catalogue —
//! so the check needs no dependency that did not already exist. A sibling crate would have
//! had to reach across for them.

use std::path::{Path, PathBuf};

/// Reads a document from the workspace, by name.
fn document(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The number a document writes immediately before a word.
///
/// Written as a scan rather than as a regular expression because the dependency policy in
/// ADR 0009 admits nothing that is not measured to earn its place, and this is twenty lines.
fn claimed(text: &str, keyword: &str) -> Option<u64> {
    let mut from = 0;
    while let Some(offset) = text[from..].find(keyword) {
        let at = from + offset;
        let before = text[..at].trim_end();
        let digits: String = before
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect();
        if !digits.is_empty() {
            let number: String = digits.chars().rev().collect();
            return number.parse().ok();
        }
        from = at + keyword.len();
    }
    None
}

/// Asserts a document claims a number, and that the number is the one given.
fn assert_claims(file: &str, keyword: &str, actual: usize) {
    let text = document(file);
    let found = claimed(&text, keyword);
    assert_eq!(
        found,
        Some(actual as u64),
        "{file} does not claim {actual} for `{keyword}`: it says {found:?}"
    );
}

#[test]
fn the_documents_count_the_tools_the_catalogue_has() {
    let tools = maj_mcp::catalog::TOOLS.len();
    assert!(tools > 0, "the catalogue is not empty");
    assert_claims("docs/MCP.md", "tools", tools);
    assert_claims("docs/COVERAGE.md", "tools", tools);
    assert_claims("README.md", "tools", tools);
}

#[test]
fn the_documents_count_the_command_kinds_the_model_has() {
    let kinds = maj_ui::KINDS.len();
    assert!(kinds > 0);
    assert_claims("docs/COVERAGE.md", "tags in `Command`", kinds);
}

#[test]
fn the_documents_count_the_strokes_the_keyboard_answers_to() {
    let strokes = maj_ui::keys::TABLE.len();
    assert!(strokes > 0);
    assert_claims("docs/UI.md", "strokes", strokes);
}

#[test]
fn every_document_the_checks_read_is_one_of_the_repositorys_own() {
    // A guard on the paths rather than on the numbers: a test that read the wrong file, or a
    // file that had been moved, would pass this suite while checking nothing at all.
    for name in ["README.md", "docs/MCP.md", "docs/COVERAGE.md", "docs/UI.md"] {
        let text = document(name);
        assert!(
            text.len() > 512,
            "{name} is {} bytes, which is too short to be the document",
            text.len()
        );
        assert!(
            Path::new(name).extension().is_some(),
            "{name} has no extension"
        );
    }
}
