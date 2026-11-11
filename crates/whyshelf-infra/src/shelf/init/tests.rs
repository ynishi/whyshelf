//! The guard of the templates: each makes, through the real adapters and in an empty
//! directory, a project that reads clean.

use whyshelf_core::app::shelf::{
    ArtifactSource, BooksCheck, Initialized, Obstacle, ShelfApp, ShelfError, ShelfRead, UpstreamEnd,
};
use whyshelf_core::domain::shelf::lookup::Reference;
use whyshelf_core::domain::shelf::schema::Schema;

use super::super::Project;
use super::super::books::parse;
use super::*;

fn project(dir: &tempfile::TempDir) -> Project {
    Project::new(dir.path(), Some("tester".to_owned()), None)
}

/// The template read and checked as the declaration of a project is.
fn declared(text: &str) -> Schema {
    let dir = tempfile::tempdir().unwrap();
    let spaces = project(&dir);
    let artifacts = spaces.artifacts();
    Schema::declared(
        parse(text).unwrap(),
        &artifacts.presets(),
        &artifacts.spaces(),
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"))
}

/// `check` of the project: sound, nothing wrong, and no gap of either strength, no
/// name with nothing behind it and none that cannot be told.
fn assert_clean(check: &BooksCheck) {
    assert!(check.is_clean(), "{check:?}");
    let BooksCheck::Checked {
        health,
        uncovered,
        absent,
        unknown,
        moved,
        ..
    } = check
    else {
        panic!("{check:?}");
    };
    assert!(health.invalid.is_empty(), "{health:?}");
    assert!(uncovered.is_empty(), "{uncovered:?}");
    assert!(absent.is_empty(), "{absent:?}");
    assert!(unknown.is_empty(), "{unknown:?}");
    assert!(moved.is_empty(), "{moved:?}");
}

#[test]
fn the_templates_are_sound_declarations_of_no_rust_preset() {
    let minimal = declared(MINIMAL);
    assert_eq!(minimal.books().len(), 1);
    assert!(minimal.book(MINIMAL_BOOK).is_some());

    let sample = declared(SAMPLE);
    let names: Vec<&str> = sample.books().keys().map(|b| b.as_str()).collect();
    for book in ["finding", "requirement", "decision", "verification"] {
        assert!(names.contains(&book), "{names:?}");
    }
    assert!(
        sample
            .book("verification")
            .unwrap()
            .artifact_links()
            .contains_key("tests")
    );
    // The sample fits a project in any language.
    assert!(!SAMPLE.contains("rust"));
    assert!(!MINIMAL.contains("rust"));
}

#[test]
fn the_sample_documents_are_requests_for_one_chain_with_no_file_named() {
    let request = sample().unwrap();
    let keys: Vec<&str> = request
        .docs
        .iter()
        .map(|d| d.key.as_deref().unwrap())
        .collect();
    assert_eq!(keys, ["F-1", "REQ-1", "DEC-1", "V-1"]);
    let v1 = &request.docs[3];
    assert!(
        v1.entries
            .contains(&("method".to_owned(), "review".to_owned()))
    );
    assert!(v1.items.iter().all(|(link, _)| link != "tests"));
    // A list is given an item at a time, as `name+=item`, so its commas are kept.
    assert!(
        request.docs[2]
            .items
            .contains(&("serves".to_owned(), "REQ-1".to_owned()))
    );
}

#[tokio::test]
async fn the_sample_makes_a_project_that_reads_clean_in_the_form_and_linked_in_one_chain() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    let Initialized { declaration, docs } = app.init(sample().unwrap()).await.unwrap();
    assert_eq!(declaration.as_str(), ".whyshelf/books.toml");
    let files: Vec<&str> = docs.iter().map(|d| d.file.as_str()).collect();
    assert_eq!(
        files,
        [
            "books/finding/F-1.md",
            "books/requirement/REQ-1.md",
            "books/decision/DEC-1.md",
            "books/verification/V-1.md"
        ]
    );
    assert!(docs.iter().all(|d| d.unresolved.is_empty()));
    // The declaration is written as it is, comments and all.
    let written = tokio::fs::read_to_string(dir.path().join(".whyshelf/books.toml"))
        .await
        .unwrap();
    assert_eq!(written, SAMPLE);

    assert_clean(&app.books_check().await.unwrap());
    let form = app.fmt_check().await.unwrap();
    assert!(form.all_in_form(), "{form:?}");
    assert_eq!(form.in_form.len(), 4);

    // Nothing is approved: every document is pending.
    let status = app.status().await.unwrap();
    assert_eq!(status.rows.len(), 4);
    assert!(status.rows.iter().all(|row| row.decision.is_none()));

    // From V-1, the links reach F-1 through DEC-1 and REQ-1.
    let trace = app
        .trace(&"V-1".parse::<Reference>().unwrap())
        .await
        .unwrap();
    let reached: Vec<String> = trace
        .upstream
        .iter()
        .filter_map(|edge| match &edge.to {
            UpstreamEnd::Doc(row) => trace.names.key_of(row.id).map(ToString::to_string),
            UpstreamEnd::Unresolved(_) | UpstreamEnd::Artifact(_) => None,
        })
        .collect();
    assert_eq!(
        reached,
        ["decision:DEC-1", "requirement:REQ-1", "finding:F-1"]
    );
}

#[tokio::test]
async fn the_minimal_declaration_makes_a_project_that_reads_clean() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    let done = app.init(minimal()).await.unwrap();
    assert!(done.docs.is_empty());
    assert_clean(&app.books_check().await.unwrap());
}

#[tokio::test]
async fn a_second_init_finds_every_file_in_the_way() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    app.init(sample().unwrap()).await.unwrap();
    let error = app.init(sample().unwrap()).await.unwrap_err();
    let ShelfError::InTheWay { obstacles } = &error else {
        panic!("{error:?}");
    };
    // The five paths, and no key: a file at its own path is told once, as a path.
    assert_eq!(obstacles.len(), 5, "{obstacles:?}");
    assert!(obstacles.iter().all(|o| matches!(o, Obstacle::There(_))));
    assert_eq!(
        *obstacles.first(),
        Obstacle::There(".whyshelf/books.toml".parse().unwrap())
    );
    // A minimal one over a sample finds the declaration alone in the way.
    let error = app.init(minimal()).await.unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with(".whyshelf/books.toml is already")
    );
}

#[test]
fn a_template_of_a_document_that_cannot_be_read_is_an_error() {
    assert!(matches!(
        doc_request("finding", "F-1.md", "no front matter"),
        Err(TemplateError::Unread { .. })
    ));
    assert!(matches!(
        doc_request("finding", "F-1.md", "---\ntitle: \"t\"\n---\n"),
        Err(TemplateError::NoKey { .. })
    ));
    let made = doc_request(
        "finding",
        "F-1.md",
        "---\nkey: \"F-1\"\nn: 3\nyes: true\n---\nBody.\n",
    )
    .unwrap();
    assert_eq!(made.key.as_deref(), Some("F-1"));
    assert_eq!(made.body, "Body.\n");
    assert!(made.entries.contains(&("n".to_owned(), "3".to_owned())));
    assert!(
        made.entries
            .contains(&("yes".to_owned(), "true".to_owned()))
    );
}
