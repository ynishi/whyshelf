//! What an approval is, is read from its record once: the verb of a decision is matched
//! for what an approval stands on, lapses at and was made by in one place of the ledger
//! (`ApprovalView::of`), and otherwise only where a record is made, written in its
//! canonical form or held against the rules of who may decide. Read from the source of
//! every crate of the workspace, the files that name `DecisionVerb` are those, each as
//! many times as listed.
//!
//! The name is counted as a token (no letter, digit or `_` on either side), in the code
//! of each file of the source but the files of tests, with its comments and the
//! contents of its literals taken out and cut where a module of tests named `tests`
//! written inline begins (`common::source::code`). So a `use` of a variant, a glob, an
//! alias and an `impl` of it are each counted where they are written. What this does
//! not see: a file that names the type through a re-export under another name (the
//! file that writes the re-export is counted, the files that use the other name are
//! not), code a macro makes, and code after a module of tests cut inline; and a module
//! of tests written inline under another name than `tests` is read as code, so it is
//! counted too much, never too little. A review reads those.

use std::collections::BTreeMap;

mod common;
use common::source::{code, is_ident, read, sources, workspace};

/// The source of every crate of the workspace.
const SOURCES: [&str; 4] = [
    "crates/whyshelf-core/src",
    "crates/whyshelf-infra/src",
    "crates/whyshelf-cli/src",
    "crates/whyshelf/src",
];

/// The files that may name `DecisionVerb`, with how many times each does: where a
/// decision is made and its type defined, written in and read from its canonical form,
/// and held against the rules of who may decide; and the ledger, which re-exports it
/// once and matches it in the view of an approval, one arm for each verb.
const NAMED: [(&str, usize); 4] = [
    ("crates/whyshelf-core/src/domain/shelf/ledger.rs", 5),
    ("crates/whyshelf-core/src/domain/shelf/ledger/admit.rs", 5),
    ("crates/whyshelf-core/src/domain/shelf/ledger/canon.rs", 14),
    (
        "crates/whyshelf-core/src/domain/shelf/ledger/decision.rs",
        9,
    ),
];

/// How many times `name` is written in `code` as a token of its own.
fn tokens(code: &str, name: &str) -> usize {
    code.match_indices(name)
        .filter(|(at, _)| {
            let before = code[..*at].chars().next_back();
            let after = code[at + name.len()..].chars().next();
            !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
        })
        .count()
}

#[test]
fn only_the_record_and_the_view_read_an_approval() {
    let root = workspace();
    let mut named: BTreeMap<String, usize> = BTreeMap::new();
    for file in SOURCES.iter().flat_map(|src| sources(&root.join(src))) {
        let count = tokens(&code(&read(&file)), "DecisionVerb");
        if count > 0 {
            let at = file
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            named.insert(at, count);
        }
    }
    let listed: BTreeMap<String, usize> = NAMED
        .iter()
        .map(|(at, count)| ((*at).to_owned(), *count))
        .collect();
    assert_eq!(named, listed);
}

#[test]
fn a_token_is_counted_with_nothing_of_a_name_on_either_side() {
    let code = "use x::DecisionVerb::{Approve, Deny};\n\
                fn f(v: &DecisionVerb) -> bool { matches!(v, DecisionVerb::Approve { .. }) }\n\
                struct DecisionVerbs; let _ = MyDecisionVerb; use DecisionVerb as V;\n";
    assert_eq!(tokens(code, "DecisionVerb"), 4);
}
