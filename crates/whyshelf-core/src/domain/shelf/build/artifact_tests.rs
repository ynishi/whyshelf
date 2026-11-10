//! Tests of the links to artifacts in a shelf (rule A1).

use crate::domain::shelf::artifact::{ArtifactHash, ArtifactState};
use crate::domain::shelf::fixtures_tests::{
    docid, fr, list, path, reading, schema, shelf, shelf_with, sp_testing, text,
};
use crate::domain::shelf::schema::ArtifactPropagation;

use super::*;

#[test]
fn a_path_is_an_artifact_there_or_not_and_never_a_link_that_cannot_be_followed() {
    let hash = ArtifactHash::of_content(b"assert!(true);\n");
    let s = shelf_with(
        vec![
            fr(1, &[]),
            sp_testing(2, &["FR-1"], &["tests/a.rs", "tests/new.rs"]),
        ],
        &[("tests/a.rs", Some(hash))],
    );
    let doc = s.doc(docid(2)).unwrap();
    let tests = doc.artifact_links().get("tests").unwrap();
    assert_eq!(tests.kind().propagation, ArtifactPropagation::Depends);
    let seen: Vec<(&str, ArtifactState)> = tests
        .artifacts()
        .iter()
        .map(|a| (a.path().as_str(), *a.state()))
        .collect();
    assert_eq!(
        seen,
        [
            ("tests/a.rs", ArtifactState::Present(hash)),
            ("tests/new.rs", ArtifactState::Absent)
        ]
    );
    // Every link to artifacts the book declares is there, written or not.
    assert!(
        doc.artifact_links()
            .get("code")
            .unwrap()
            .artifacts()
            .is_empty()
    );
    assert_eq!(doc.artifact_links_of(ArtifactPropagation::Depends).len(), 1);
}

#[test]
fn a_text_that_is_no_path_makes_the_document_invalid() {
    let s = shelf(vec![sp_testing(2, &[], &["/etc/passwd", "../up.rs"])]);
    let DocState::Invalid(files) = s.state_of(docid(2)) else {
        panic!("a path out of the root makes the document invalid");
    };
    let problems: Vec<&DocProblem> = files.iter().flat_map(|f| f.problems().iter()).collect();
    assert_eq!(problems.len(), 2);
    assert!(problems.iter().all(
        |p| matches!(p, DocProblem::ArtifactPathInvalid { link, .. } if link.as_str() == "tests")
    ));
}

#[test]
fn a_link_to_artifacts_holds_a_list_of_the_count_it_declares() {
    let s = shelf(vec![reading(
        "specs/sp-2.md",
        2,
        "SP-2",
        &[("title", text("t")), ("tests", text("tests/a.rs"))],
    )]);
    let DocState::Invalid(files) = s.state_of(docid(2)) else {
        panic!("a link that is not a list makes the document invalid");
    };
    assert!(matches!(
        files.first().problems().first(),
        DocProblem::LinkNotList { .. }
    ));
}

#[test]
fn the_paths_to_read_are_those_the_files_under_a_book_write() {
    let readings = vec![
        sp_testing(2, &[], &["tests/a.rs", "tests/b.rs"]),
        sp_testing(3, &[], &["tests/a.rs", "/abs"]),
        // Under no book: what it writes is not read.
        reading(
            "elsewhere/x.md",
            4,
            "SP-4",
            &[("tests", list(&["tests/c.rs"]))],
        ),
    ];
    let paths = Shelf::artifact_paths(&schema(), &readings);
    assert_eq!(
        paths.into_iter().collect::<Vec<_>>(),
        [path("tests/a.rs"), path("tests/b.rs")]
    );
}

#[test]
fn the_hash_of_a_document_does_not_cover_its_links_to_artifacts() {
    let a = shelf(vec![sp_testing(2, &[], &["tests/a.rs"])]);
    let b = shelf(vec![sp_testing(2, &[], &["tests/b.rs"])]);
    assert_eq!(
        a.doc(docid(2)).unwrap().hash(),
        b.doc(docid(2)).unwrap().hash()
    );
}
