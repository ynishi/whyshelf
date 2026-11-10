//! Tests of the links to artifacts in a shelf (rule A1).

use crate::domain::shelf::artifact::{ArtifactHash, ArtifactReading, ArtifactState};
use crate::domain::shelf::fixtures_tests::{
    docid, file_artifact, file_space, fr, list, name, reading, schema, shelf, shelf_answered,
    shelf_with, sp_testing, text,
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
        .map(|a| (a.name().as_str(), a.state().clone()))
        .collect();
    assert!(tests.artifacts().iter().all(|a| *a.space() == file_space()));
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
fn a_text_the_adapter_says_is_no_name_makes_the_document_invalid() {
    let no_path = || ArtifactReading::not_a_name(NonBlank::new("no path under the root").unwrap());
    let s = shelf_answered(
        vec![sp_testing(
            2,
            &[],
            &["/etc/passwd", "../up.rs", "tests/a.rs"],
        )],
        &[
            ("/etc/passwd", no_path()),
            ("../up.rs", no_path()),
            ("tests/a.rs", ArtifactReading::absent()),
        ],
    );
    let DocState::Invalid(files) = s.state_of(docid(2)) else {
        panic!("a text that is no name of the space makes the document invalid");
    };
    let problems: Vec<&DocProblem> = files.iter().flat_map(|f| f.problems().iter()).collect();
    assert_eq!(problems.len(), 2);
    assert!(problems.iter().all(|p| matches!(
        p,
        DocProblem::NotAnArtifactName { link, space, why, .. }
            if link.as_str() == "tests" && *space == file_space()
                && why.as_str() == "no path under the root"
    )));
    assert!(
        problems
            .iter()
            .any(|p| p.to_string().contains("`../up.rs` in the link `tests`"))
    );
}

#[test]
fn a_text_that_is_no_name_makes_the_document_invalid() {
    let too_long = "a".repeat(crate::domain::shelf::artifact::NAME_MAX + 1);
    let s = shelf(vec![sp_testing(2, &[], &["", &too_long, "a\tb"])]);
    let DocState::Invalid(files) = s.state_of(docid(2)) else {
        panic!("a text that is no name makes the document invalid");
    };
    let whys: Vec<ArtifactNameProblem> = files
        .iter()
        .flat_map(|f| f.problems().iter())
        .filter_map(|p| match p {
            DocProblem::ArtifactNameInvalid { link, why, .. } if link.as_str() == "tests" => {
                Some(*why)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        whys,
        [
            ArtifactNameProblem::Empty,
            ArtifactNameProblem::TooLong {
                bytes: crate::domain::shelf::artifact::NAME_MAX + 1
            },
            ArtifactNameProblem::ControlChar
        ]
    );
}

#[test]
fn a_name_the_adapter_gave_no_answer_for_cannot_be_told_and_is_never_absent() {
    let s = Shelf::build(
        &schema(),
        vec![sp_testing(2, &[], &["tests/a.rs"])],
        &ArtifactReadings::new(),
    );
    let doc = s.doc(docid(2)).unwrap();
    assert!(doc.absent_artifacts().is_empty());
    let unknown: Vec<_> = doc
        .unknown_artifacts()
        .into_iter()
        .map(|(_, _, why)| why.clone())
        .collect();
    assert_eq!(unknown, [crate::domain::shelf::artifact::Untold::NoAnswer]);
}

#[test]
fn a_name_the_adapter_cannot_tell_about_is_an_unknown_artifact_of_a_valid_document() {
    let why = NonBlank::new("offline").unwrap();
    let s = shelf_answered(
        vec![sp_testing(2, &[], &["tests/a.rs", "tests/b.rs"])],
        &[
            ("tests/a.rs", ArtifactReading::cannot_tell(why.clone())),
            ("tests/b.rs", ArtifactReading::absent()),
        ],
    );
    let doc = s.doc(docid(2)).unwrap();
    let unknown: Vec<_> = doc
        .unknown_artifacts()
        .into_iter()
        .map(|(link, artifact, why)| (link.as_str(), artifact.clone(), why.clone()))
        .collect();
    assert_eq!(
        unknown,
        [(
            "tests",
            file_artifact("tests/a.rs"),
            crate::domain::shelf::artifact::Untold::Said(why.clone())
        )]
    );
    // The absent ones are another list.
    let absent: Vec<_> = doc
        .absent_artifacts()
        .into_iter()
        .map(|(_, artifact)| artifact.clone())
        .collect();
    assert_eq!(absent, [file_artifact("tests/b.rs")]);
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
    let asks = Shelf::artifact_asks(&schema(), &readings);
    // Every name is asked, in its link's space: whether `/abs` is a name of the space
    // is the adapter's answer.
    assert_eq!(
        asks.iter()
            .map(|(space, name, _)| (space.clone(), name.clone()))
            .collect::<Vec<_>>(),
        [
            (file_space(), name("/abs")),
            (file_space(), name("tests/a.rs")),
            (file_space(), name("tests/b.rs"))
        ]
    );
    // Each name with the comparison of the link that writes it.
    assert!(asks.iter().all(|(_, _, c)| {
        c.iter()
            .eq([&crate::domain::shelf::artifact::Comparison::Bytes])
    }));
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
