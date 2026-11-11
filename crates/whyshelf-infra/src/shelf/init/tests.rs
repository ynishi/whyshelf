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
        Template::Minimal | Template::Sample | Template::Experiment => Vec::new(),
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
    assert_eq!(names, ["minimal", "sample", "qa-first", "experiment"]);
    for template in Template::ALL {
        assert_eq!(Template::named(template.name()), Some(template));
        assert!(!template.about().is_empty());
    }
    assert_eq!(Template::named("qa_first"), None);

    // `ALL` lists every template once, in place: a template added fails to compile
    // here until it has an arm, and the arms are counted against `ALL`.
    const ARMS: usize = 4;
    let place = |template: Template| match template {
        Template::Minimal => 0,
        Template::Sample => 1,
        Template::QaFirst => 2,
        Template::Experiment => 3,
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
            Template::Sample | Template::QaFirst | Template::Experiment => {
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
        for (name, text) in template.files() {
            assert!(!text.contains("rust"), "{template:?} {name}");
        }
    }
}

#[test]
fn experiment_declares_a_decision_s_code_and_both_halves_of_a_measurement() {
    use whyshelf_core::domain::shelf::schema::{Cardinality, FieldType, Propagation};
    let schema = declared(EXPERIMENT);

    // A decision's code: files, pinned by their being there only.
    let decision = schema.book("decision").unwrap();
    let code = decision.artifact_links().get("code").unwrap();
    assert_eq!(code.space.as_str(), "file");
    assert_eq!(code.comparison.kind_word(), "presence");
    assert_eq!(code.propagation.propagation(), Propagation::Depends);
    assert_eq!(code.cardinality, Cardinality::Many);
    assert_eq!(
        decision.preset_of("code").map(|p| p.name().as_str()),
        Some("code")
    );

    let verification = schema.book("verification").unwrap();
    let method = verification.fields().get("method").unwrap();
    let FieldType::Enum(words) = &method.ty else {
        panic!("{method:?}");
    };
    let words: std::collections::BTreeSet<&str> = words.iter().map(|w| w.as_str()).collect();
    assert_eq!(
        words,
        ["test", "review", "plan", "result"].into_iter().collect()
    );
    // The sheet and the log: files, every byte pinned, neither required of a
    // verification, so a plan needs no log and a result no sheet.
    for name in ["sheet", "log"] {
        let link = verification.artifact_links().get(name).unwrap();
        assert_eq!(link.space.as_str(), "file", "{name}");
        assert_eq!(link.comparison.kind_word(), "bytes", "{name}");
        assert_eq!(link.propagation.propagation(), Propagation::Depends);
        assert_eq!(link.cardinality, Cardinality::Many, "{name}");
    }
    // A result stands on its plan, one at most; a plan names none.
    let plans = verification.links().get("plans").unwrap();
    assert_eq!(plans.target.as_str(), "verification");
    assert_eq!(plans.propagation, Propagation::Depends);
    assert_eq!(plans.cardinality, Cardinality::Optional);
    // A result names the decision its plan checks: `checks` is required, as in
    // the sample.
    let checks = verification.links().get("checks").unwrap();
    assert_eq!(checks.cardinality, Cardinality::AtLeastOne);
    assert!(checks.cover.is_some());
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

/// The head of [`EXPERIMENT`] on its flow, by its exact text: the two lines added
/// to the drawing, and the paragraphs on the flow and its conventions.
const EXPERIMENT_HEAD: [&str; 2] = [
    "#                                                                   ^ plans\n\
     #                                                          (a result, to its plan)\n",
    "# This is for checks that are measurements registered before they are run. A\n\
     # plan writes down what will be measured and a table of what is expected, and is\n\
     # approved before the run; after the run, a result links the plan and the record\n\
     # of the run, and is approved then. They are two verifications, not one: one\n\
     # approval stands per document, so the record of the run added to the plan and\n\
     # approved would stand in the place of the approval made before the run, and the\n\
     # plan approved again unchanged is left out as `repeats`.\n\
     #\n\
     # Which half names what, and the order of the approvals, are conventions of this\n\
     # template: `check` does not enforce them. A result may name a sheet, a plan a\n\
     # log, and `plans` a verification that is no plan; a result may be approved\n\
     # before its plan, as `plans` pins the plan's document, not its approval. The\n\
     # ledger records the time of each approval, so a reader can see the order;\n\
     # whyshelf does not refuse the other order, and the ledger does not record when\n\
     # the run happened. Each choice made for this flow is marked \"experiment\" below.\n\
     #\n",
];

/// The tables [`EXPERIMENT`] adds, each by its exact text.
const EXPERIMENT_TABLES: [&str; 4] = [
    "[[book.link]]\nname = \"code\"\ntarget = \"artifact\"\n\
     preset = \"code\"             # space = \"file\", compare = \"presence\", means = \"depends\"\n",
    "[[book.link]]\nname = \"sheet\"\ntarget = \"artifact\"\npreset = \"file\"\ncount = \"many\"\n",
    "[[book.link]]\nname = \"log\"\ntarget = \"artifact\"\npreset = \"file\"\ncount = \"many\"\n",
    "[[book.link]]\nname = \"plans\"\ntarget = \"verification\"\ncount = \"optional\"\nmeans = \"depends\"\n",
];

/// `text` with what [`EXPERIMENT`] adds to [`SAMPLE`] taken out, and the method's
/// words put back as the sample has them; none when a part is not there exactly
/// once. Only these are taken out: the head by its exact text, the comment lines
/// of each block that starts `# experiment:` (comments only, never a key), and
/// the four tables by their exact text.
fn without_experiment(text: &str) -> Option<String> {
    let once = |text: &str, part: &str| text.matches(part).count() == 1;
    let mut text = text.to_owned();
    for part in EXPERIMENT_HEAD {
        if !once(&text, part) {
            return None;
        }
        text = text.replacen(part, "", 1);
    }
    // The comment lines of each marked block: a run of whole-line comments whose
    // first line starts `# experiment:`.
    let mut kept = String::new();
    let mut in_block = false;
    let mut blocks = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("# experiment:") {
            in_block = true;
            blocks += 1;
            continue;
        }
        if in_block && line.starts_with('#') {
            continue;
        }
        in_block = false;
        kept.push_str(line);
    }
    if blocks != 5 {
        return None;
    }
    let mut text = kept;
    for table in EXPERIMENT_TABLES {
        if !once(&text, table) {
            return None;
        }
        text = text.replacen(table, "", 1);
    }
    let words = "type = { enum = [\"test\", \"review\", \"plan\", \"result\"] }\n";
    if !once(&text, words) {
        return None;
    }
    Some(text.replacen(words, "type = { enum = [\"test\", \"review\"] }\n", 1))
}

#[test]
fn experiment_is_sample_with_only_the_parts_marked_experiment_added() {
    assert_eq!(without_experiment(EXPERIMENT).as_deref(), Some(SAMPLE));

    // Each edit outside the marked parts is caught.
    let appended = format!("{EXPERIMENT}[[book.link]]\nname = \"extra\"\ntarget = \"decision\"\n");
    let between = EXPERIMENT.replacen(
        "# experiment: a result's record",
        "[[book.link]]\nname = \"between\"\ntarget = \"artifact\"\n# experiment: a result's record",
        1,
    );
    let key_after_comment = EXPERIMENT.replacen(
        "# the run gave, after it.\n",
        "# the run gave, after it.\nunique = true\n",
        1,
    );
    let header_line = EXPERIMENT.replacen("# checked.\n", "# checked.\n# One more line.\n", 1);
    for (edit, text) in [
        ("an appended link", appended),
        ("a link between sheet and log", between),
        ("a key after the method's comment", key_after_comment),
        ("a line in the header", header_line),
    ] {
        assert_ne!(text, EXPERIMENT, "{edit}: the edit was not made");
        assert_ne!(without_experiment(&text).as_deref(), Some(SAMPLE), "{edit}");
    }
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
        // The other files too, as they are, so no name a document writes has
        // nothing behind it.
        let files: Vec<&str> = done.files.iter().map(|f| f.as_str()).collect();
        let meant: Vec<&str> = template.files().iter().map(|(path, _)| *path).collect();
        assert_eq!(files, meant);
        for (path, text) in template.files() {
            let there = tokio::fs::read_to_string(dir.path().join(path))
                .await
                .unwrap();
            assert_eq!(there, *text, "{template:?} {path}");
        }

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

/// Approve these documents in one run, in this order (each after what it stands on).
async fn approve(app: &mut ShelfApp<Project>, keys: &[&str]) {
    use whyshelf_core::app::shelf::{JudgeRequest, Judgement};
    let requests = keys
        .iter()
        .map(|key| JudgeRequest {
            judgement: Judgement::Approve {
                until: None,
                reason: None,
                again: false,
            },
            reference: key.parse().unwrap(),
            actor: None,
        })
        .collect();
    app.judge_all(requests).await.unwrap();
}

/// Where the document of this key stands now.
async fn standing(
    app: &ShelfApp<Project>,
    key: &str,
) -> whyshelf_core::domain::shelf::freshness::StandingKind {
    let report = app.get(&key.parse::<Reference>().unwrap()).await.unwrap();
    report.row.standing.kind()
}

#[tokio::test]
async fn experiment_writes_a_plan_with_its_table_and_takes_a_result_with_no_sheet() {
    use whyshelf_core::domain::shelf::freshness::{FreshnessLevel, StandingKind};
    let request = Template::Experiment.request().unwrap();
    let keys: Vec<&str> = request
        .docs
        .iter()
        .map(|d| d.key.as_deref().unwrap())
        .collect();
    assert_eq!(keys, ["F-1", "REQ-1", "DEC-1", "V-1"]);
    // V-1 is a plan with its table, and no log: the run has not happened.
    let v1 = &request.docs[3];
    assert!(
        v1.entries
            .contains(&("method".to_owned(), "plan".to_owned()))
    );
    assert!(
        v1.items
            .contains(&("sheet".to_owned(), "measurements/save-time.csv".to_owned()))
    );
    assert!(v1.items.iter().all(|(link, _)| link != "log"));
    // DEC-1 names no code yet: none is written.
    assert!(request.docs[2].items.iter().all(|(link, _)| link != "code"));

    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    let Initialized { heads, .. } = app.init(request).await.unwrap();
    // What `next` approves: F-1 (only grounded on) and V-1, the plan, which stands
    // on DEC-1 and REQ-1.
    let heads: Vec<String> = heads.iter().map(|k| k.form.to_string()).collect();
    assert_eq!(heads, ["F-1", "V-1"]);
    assert_eq!(
        reached(&app, "V-1").await,
        ["decision:DEC-1", "requirement:REQ-1", "finding:F-1"]
    );

    // Approved before the run, the plan, which names no log, holds.
    approve(&mut app, &["F-1", "REQ-1", "DEC-1", "V-1"]).await;
    let fresh = StandingKind::Approved(FreshnessLevel::Fresh);
    assert_eq!(standing(&app, "V-1").await, fresh);

    // After the run: its log, and a result that links the decision, its plan and
    // the log, with no sheet, is valid, and the check stays clean.
    tokio::fs::write(dir.path().join("measurements/run-1.txt"), "1 MB: 0.04 s\n")
        .await
        .unwrap();
    let result = |items: &[(&str, &str)]| NewDocRequest {
        book: BookName::new("verification").unwrap(),
        key: None,
        entries: vec![
            (
                "title".to_owned(),
                "Result: the four sizes timed".to_owned(),
            ),
            ("method".to_owned(), "result".to_owned()),
        ],
        items: items
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        body: String::new(),
    };
    // One that names no decision is refused: `checks` is required, as in the
    // sample, so no verification checks nothing.
    let refused = app
        .new_doc(result(&[
            ("plans", "V-1"),
            ("log", "measurements/run-1.txt"),
        ]))
        .await
        .unwrap_err();
    assert!(matches!(refused, ShelfError::NewDoc(_)), "{refused:?}");
    let made = app
        .new_doc(result(&[
            ("checks", "DEC-1"),
            ("plans", "V-1"),
            ("log", "measurements/run-1.txt"),
        ]))
        .await
        .unwrap();
    assert_eq!(made.key.form.to_string(), "V-2");
    assert!(made.unresolved.is_empty());
    assert_clean(&app.books_check().await.unwrap(), &[]);
    assert!(
        reached(&app, "V-2")
            .await
            .contains(&"verification:V-1".to_owned())
    );
    approve(&mut app, &["V-2"]).await;
    assert_eq!(standing(&app, "V-2").await, fresh);

    // The table edited after the run: the plan is stale, and the result with it.
    tokio::fs::write(
        dir.path().join("measurements/save-time.csv"),
        "size_mb,saves,slowest_expected_seconds\n1,5,0.5\n",
    )
    .await
    .unwrap();
    let stale = StandingKind::Approved(FreshnessLevel::Stale);
    assert_eq!(standing(&app, "V-1").await, stale);
    assert_eq!(standing(&app, "V-2").await, stale);
}

/// `init` of `template` in a project at `outer/project` whose `link` is a link to
/// `outer/out`: refused, the link named, and nothing written outside the root.
#[cfg(unix)]
async fn refused_through_a_link_out(template: Template, link: &str) {
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("project");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::create_dir_all(outer.path().join("out"))
        .await
        .unwrap();
    tokio::fs::symlink("../out", root.join(link)).await.unwrap();
    let mut app = ShelfApp::new(Project::new(&root, Some("tester".to_owned()), None));
    let error = app.init(template.request().unwrap()).await.unwrap_err();
    let ShelfError::InTheWay { obstacles } = &error else {
        panic!("{template:?}: {error:?}");
    };
    assert!(
        obstacles
            .iter()
            .any(|o| *o == Obstacle::There(link.parse().unwrap())),
        "{template:?}: {obstacles:?}"
    );
    let mut out = tokio::fs::read_dir(outer.path().join("out")).await.unwrap();
    assert!(out.next_entry().await.unwrap().is_none(), "{template:?}");
    assert!(
        tokio::fs::symlink_metadata(root.join(".whyshelf"))
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_table_or_a_document_is_never_written_through_a_link_out_of_the_root() {
    // The table of experiment, under `measurements`.
    refused_through_a_link_out(Template::Experiment, "measurements").await;
    // The documents of sample, under `books`.
    refused_through_a_link_out(Template::Sample, "books").await;
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
    // An experiment over a sample finds five: the declaration and the four
    // documents (the table is not there).
    let error = app
        .init(Template::Experiment.request().unwrap())
        .await
        .unwrap_err();
    let ShelfError::InTheWay { obstacles } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(obstacles.len(), 5, "{obstacles:?}");
    assert!(
        !obstacles
            .iter()
            .any(|o| *o == Obstacle::There("measurements/save-time.csv".parse().unwrap()))
    );
    // Over an experiment, the table is in the way too.
    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    app.init(Template::Experiment.request().unwrap())
        .await
        .unwrap();
    let error = app
        .init(Template::Experiment.request().unwrap())
        .await
        .unwrap_err();
    let ShelfError::InTheWay { obstacles } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(obstacles.len(), 6, "{obstacles:?}");
    assert_eq!(
        obstacles.iter().nth(1),
        Some(&Obstacle::There(
            "measurements/save-time.csv".parse().unwrap()
        ))
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
