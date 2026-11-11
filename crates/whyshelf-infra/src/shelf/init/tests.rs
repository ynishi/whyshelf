//! The guard of the templates: each makes, through the real adapters and in an empty
//! directory, a project that reads clean.

use whyshelf_core::app::shelf::{
    ArtifactSource, BooksCheck, Initialized, NewDocRequest, Obstacle, ShelfApp, ShelfError,
    ShelfRead, UpstreamEnd,
};
use whyshelf_core::domain::shelf::lookup::Reference;
use whyshelf_core::domain::shelf::schema::{BookName, CoverSeverity, Schema};

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

/// The gaps a template means `check` to list right after `init`: `(key, book,
/// link, severity)`.
fn meant_gaps(
    template: Template,
) -> Vec<(&'static str, &'static str, &'static str, CoverSeverity)> {
    match template {
        Template::Minimal | Template::Sample => Vec::new(),
        // The requirement is written before any decision serves it.
        Template::QaFirst => vec![("REQ-1", "decision", "serves", CoverSeverity::Warn)],
    }
}

/// `check` of the project: sound, nothing wrong, the gaps `meant` and no other, no
/// name with nothing behind it and none that cannot be told.
fn assert_clean(check: &BooksCheck, meant: &[(&str, &str, &str, CoverSeverity)]) {
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
    let gaps: Vec<(String, &str, &str, CoverSeverity)> = uncovered
        .iter()
        .map(|u| {
            (
                u.key.as_ref().unwrap().form.to_string(),
                u.book.as_str(),
                u.link.as_str(),
                u.severity,
            )
        })
        .collect();
    let meant: Vec<(String, &str, &str, CoverSeverity)> = meant
        .iter()
        .map(|&(key, book, link, severity)| (key.to_owned(), book, link, severity))
        .collect();
    assert_eq!(gaps, meant);
    assert!(absent.is_empty(), "{absent:?}");
    assert!(unknown.is_empty(), "{unknown:?}");
    assert!(moved.is_empty(), "{moved:?}");
}

#[test]
fn the_templates_are_named_once_and_found_by_their_names() {
    let names: Vec<&str> = Template::ALL.iter().map(|t| t.name()).collect();
    assert_eq!(names, ["minimal", "sample", "qa-first"]);
    for template in Template::ALL {
        assert_eq!(Template::named(template.name()), Some(template));
        assert!(!template.about().is_empty());
    }
    assert_eq!(Template::named("qa_first"), None);

    // `ALL` lists every template once, in place: a template added fails to compile
    // here until it has an arm, and the arms are counted against `ALL`.
    const ARMS: usize = 3;
    let place = |template: Template| match template {
        Template::Minimal => 0,
        Template::Sample => 1,
        Template::QaFirst => 2,
    };
    assert_eq!(Template::ALL.len(), ARMS);
    for (at, template) in Template::ALL.into_iter().enumerate() {
        assert_eq!(place(template), at, "{template:?}");
    }
    assert_eq!(Template::default(), Template::Minimal);
}

#[test]
fn every_template_is_a_sound_declaration_with_no_rust_in_it() {
    for template in Template::ALL {
        let schema = declared(template.declaration());
        let books: Vec<&str> = schema.books().keys().map(|b| b.as_str()).collect();
        match template {
            Template::Minimal => {
                assert_eq!(books, [MINIMAL_BOOK]);
                assert_eq!(template.first_book(), Some(MINIMAL_BOOK));
            }
            Template::Sample | Template::QaFirst => {
                for book in ["finding", "requirement", "decision", "verification"] {
                    assert!(books.contains(&book), "{template:?}: {books:?}");
                }
                assert!(
                    schema
                        .book("verification")
                        .unwrap()
                        .artifact_links()
                        .contains_key("tests")
                );
                assert_eq!(template.first_book(), None);
            }
        }
        // A template fits a project in any language.
        assert!(!template.declaration().contains("rust"), "{template:?}");
        for (_, name, text) in template.docs() {
            assert!(!text.contains("rust"), "{template:?} {name}");
        }
    }
}

#[test]
fn qa_first_is_sample_with_the_two_rules_loosened_and_commented() {
    // Take out the paragraph of the head on the flow, put the two rules and the
    // verification's description back as the sample has them, comments and all,
    // and the whole text is the sample's.
    let head = "# This is for a flow where the checks come first: findings, requirements and\n\
                # their verifications are written now, and the decisions at release. Two rules\n\
                # are loose for that flow, each marked \"qa-first\" below; tighten both at release.\n\
                #\n";
    let description =
        "description = \"how a requirement's promise is checked, before or after the decision\"\n";
    let checks = "# qa-first: a verification may check no decision yet. At release, set it back to\n\
                  # `count = \"at-least-one\"`, so every verification must check a decision.\n\
                  count = \"many\"\n";
    let serves = "# qa-first: a requirement no decision serves yet is listed by `check`, not a\n\
                  # failure. At release, set it back to `cover = true`, so every requirement must\n\
                  # have a decision that serves it.\n\
                  cover = \"warn\"\n";
    for part in [head, description, checks, serves] {
        assert_eq!(QA_FIRST.matches(part).count(), 1, "{part}");
    }
    let tightened = QA_FIRST
        .replace(head, "")
        .replace(description, "description = \"how a decision is checked\"\n")
        .replace(checks, "count = \"at-least-one\"\n")
        .replace(
            serves,
            "cover = true                # every requirement is expected to have a decision that serves it\n",
        );
    assert_eq!(tightened, SAMPLE);
}

#[tokio::test]
async fn every_template_makes_a_project_that_reads_clean_in_the_form_and_pending() {
    for template in Template::ALL {
        let dir = tempfile::tempdir().unwrap();
        let mut app = ShelfApp::new(project(&dir));
        let request = template.request().unwrap();
        let count = request.docs.len();
        assert_eq!(count, template.docs().len());
        let done = app.init(request).await.unwrap();
        assert_eq!(done.declaration.as_str(), ".whyshelf/books.toml");
        assert_eq!(done.docs.len(), count);
        assert!(done.docs.iter().all(|d| d.unresolved.is_empty()));
        // The declaration is written as it is, comments and all.
        let written = tokio::fs::read_to_string(dir.path().join(".whyshelf/books.toml"))
            .await
            .unwrap();
        assert_eq!(written, template.declaration());

        assert_clean(&app.books_check().await.unwrap(), &meant_gaps(template));
        let form = app.fmt_check().await.unwrap();
        assert!(form.all_in_form(), "{template:?}: {form:?}");
        assert_eq!(form.in_form.len(), count);

        // Nothing is approved: every document is pending.
        let status = app.status().await.unwrap();
        assert_eq!(status.rows.len(), count);
        assert!(status.rows.iter().all(|row| row.decision.is_none()));
    }
}

/// The keys of the documents the links reach from `reference`, upstream.
async fn reached(app: &ShelfApp<Project>, reference: &str) -> Vec<String> {
    let trace = app
        .trace(&reference.parse::<Reference>().unwrap())
        .await
        .unwrap();
    trace
        .upstream
        .iter()
        .filter_map(|edge| match &edge.to {
            UpstreamEnd::Doc(row) => trace.names.key_of(row.id).map(ToString::to_string),
            UpstreamEnd::Unresolved(_) | UpstreamEnd::Artifact(_) => None,
        })
        .collect()
}

#[tokio::test]
async fn the_sample_documents_are_one_chain_with_no_file_named() {
    let request = Template::Sample.request().unwrap();
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

    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    let Initialized { docs, heads, .. } = app.init(request).await.unwrap();
    // F-1 is stood on by no `depends` link (REQ-1 only grounds on it), V-1 by none.
    let heads: Vec<String> = heads.iter().map(|k| k.form.to_string()).collect();
    assert_eq!(heads, ["F-1", "V-1"]);
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
    // From V-1, the links reach F-1 through DEC-1 and REQ-1.
    assert_eq!(
        reached(&app, "V-1").await,
        ["decision:DEC-1", "requirement:REQ-1", "finding:F-1"]
    );
}

#[tokio::test]
async fn qa_first_writes_a_verification_that_checks_no_decision_and_takes_another() {
    let request = Template::QaFirst.request().unwrap();
    let keys: Vec<&str> = request
        .docs
        .iter()
        .map(|d| d.key.as_deref().unwrap())
        .collect();
    assert_eq!(keys, ["F-1", "REQ-1", "V-1"]);
    let v1 = &request.docs[2];
    assert!(
        v1.entries
            .contains(&("method".to_owned(), "review".to_owned()))
    );
    assert!(
        v1.items
            .iter()
            .chain(&v1.entries)
            .all(|(link, _)| link != "checks" && link != "tests")
    );

    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    let Initialized { heads, .. } = app.init(request).await.unwrap();
    // No `depends` link stands on any of the three: REQ-1 only grounds on F-1, and
    // V-1 checks nothing yet.
    let heads: Vec<String> = heads.iter().map(|k| k.form.to_string()).collect();
    assert_eq!(heads, ["F-1", "REQ-1", "V-1"]);
    assert_eq!(reached(&app, "REQ-1").await, ["finding:F-1"]);
    assert!(reached(&app, "V-1").await.is_empty());

    // Another verification with no decision is valid there too, and the check stays
    // as it was.
    let made = app
        .new_doc(NewDocRequest {
            book: BookName::new("verification").unwrap(),
            key: None,
            entries: vec![
                ("title".to_owned(), "Save twice at once".to_owned()),
                ("method".to_owned(), "review".to_owned()),
            ],
            items: Vec::new(),
            body: String::new(),
        })
        .await
        .unwrap();
    assert_eq!(made.key.form.to_string(), "V-2");
    assert_clean(
        &app.books_check().await.unwrap(),
        &meant_gaps(Template::QaFirst),
    );
}

#[tokio::test]
async fn a_second_init_finds_every_file_in_the_way() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    app.init(Template::Sample.request().unwrap()).await.unwrap();
    let error = app
        .init(Template::Sample.request().unwrap())
        .await
        .unwrap_err();
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
    let error = app
        .init(Template::Minimal.request().unwrap())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with(".whyshelf/books.toml is already")
    );
    // A qa-first one over a sample finds its four paths, the declaration first.
    let error = app
        .init(Template::QaFirst.request().unwrap())
        .await
        .unwrap_err();
    let ShelfError::InTheWay { obstacles } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(obstacles.len(), 4, "{obstacles:?}");
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
