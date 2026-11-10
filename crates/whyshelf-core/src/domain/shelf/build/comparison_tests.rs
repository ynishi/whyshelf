//! Tests of the comparisons the build asks of each path, and of the hash each link
//! to artifacts takes.

use std::collections::BTreeSet;

use crate::domain::shelf::artifact::{ArtifactState, Comparison};
use crate::domain::shelf::fixtures_tests::{
    docid, lines, list, path, reading, schema_of_lines, shelf_reading, text,
};
use crate::domain::shelf::schema::ArtifactPropagation;

use super::*;

/// SP-`n`, tested by `tests` and with `code`.
fn spec(n: u128, tests: &[&str], code: &[&str]) -> DocReading {
    reading(
        &format!("specs/sp-{n}.md"),
        n,
        &format!("SP-{n}"),
        &[
            ("title", text("spec")),
            ("tests", list(tests)),
            ("code", list(code)),
        ],
    )
}

#[test]
fn each_path_is_asked_with_the_comparisons_of_the_links_that_write_it() {
    let schema = schema_of_lines(&["//"]);
    let readings = vec![
        spec(2, &["src/a.rs"], &["src/a.rs", "src/b.rs"]),
        spec(3, &["src/c.rs"], &[]),
    ];
    let asks = Shelf::artifact_asks(&schema, &readings);
    let slashes = lines(&["//"]);
    assert_eq!(
        asks,
        ArtifactAsks::from([
            (
                path("src/a.rs"),
                BTreeSet::from([Comparison::Bytes, slashes.clone()])
            ),
            (path("src/b.rs"), BTreeSet::from([Comparison::Bytes])),
            (path("src/c.rs"), BTreeSet::from([slashes])),
        ])
    );
}

#[test]
fn each_link_takes_the_hash_of_its_own_comparison_of_one_file() {
    let schema = schema_of_lines(&["//"]);
    let bytes = b"// note\nfn a() {}\n";
    let shelf = shelf_reading(
        &schema,
        vec![spec(2, &["src/a.rs"], &["src/a.rs"])],
        &[("src/a.rs", bytes)],
    );
    let doc = shelf.doc(docid(2)).unwrap();
    let state_under = |propagation| {
        let links = doc.artifact_links_of(propagation);
        let [(_, artifacts)] = links.as_slice() else {
            panic!("one link: {links:?}");
        };
        *artifacts[0].state()
    };
    assert_eq!(
        state_under(ArtifactPropagation::Depends),
        ArtifactState::Present(lines(&["//"]).fingerprint(bytes))
    );
    assert_eq!(
        state_under(ArtifactPropagation::Informs),
        ArtifactState::Present(Comparison::Bytes.fingerprint(bytes))
    );
}
