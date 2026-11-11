//! Tests of the range of a book: what it takes, what the declaration refuses, and
//! what a file with no front matter becomes.

use super::*;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::{
    ClaimKeyProblem, DocReading, NextKeyProblem, RawProblem, ReadId, Shelf, SkipWhy,
};
use crate::domain::shelf::fixtures_tests::{docid, entries, path, spaces, text};

fn scoped(include: Option<&[&str]>, exclude: &[&str], skip: Option<&str>) -> ScopeDecl {
    ScopeDecl {
        include: include.map(|list| list.iter().map(|s| (*s).to_owned()).collect()),
        exclude: (!exclude.is_empty()).then(|| exclude.iter().map(|s| (*s).to_owned()).collect()),
        without_front_matter: skip.map(str::to_owned),
    }
}

fn book_with(name: &str, dir: &str, prefix: &str, scope: ScopeDecl) -> BookSchemaDecl {
    BookSchemaDecl {
        name: name.to_owned(),
        dir: dir.to_owned(),
        scope,
        key: KeyPatternDecl::Prefixed {
            prefix: prefix.to_owned(),
        },
        description: None,
        fields: Vec::new(),
        links: Vec::new(),
    }
}

fn schema_of(scope: ScopeDecl) -> Schema {
    Schema::validate(vec![book_with("spec", "docs/spec", "SP", scope)], &spaces()).unwrap()
}

fn problems(scope: ScopeDecl) -> Vec<SchemaProblem> {
    Schema::validate(vec![book_with("spec", "docs/spec", "SP", scope)], &spaces())
        .unwrap_err()
        .into_vec()
}

/// Whether the book `spec` of `docs/spec` takes `file`.
fn takes(schema: &Schema, file: &str) -> bool {
    schema.book("spec").unwrap().takes(&path(file))
}

#[test]
fn with_no_setting_a_book_takes_every_markdown_file_below_its_directory() {
    let schema = schema_of(ScopeDecl::default());
    assert!(takes(&schema, "docs/spec/SP-1.md"));
    assert!(takes(&schema, "docs/spec/deep/er/a.md"));
    // Not a Markdown file, a hidden name with no stem, or not below the directory.
    assert!(!takes(&schema, "docs/spec/a.txt"));
    assert!(!takes(&schema, "docs/spec/a.md.bak"));
    assert!(!takes(&schema, "docs/spec/.md"));
    assert!(!takes(&schema, "docs/spec"));
    assert!(!takes(&schema, "docs/spec-old/a.md"));
    assert!(!takes(&schema, "docs/other/a.md"));
}

#[test]
fn an_include_narrows_the_range_and_a_star_stops_at_a_slash() {
    let schema = schema_of(scoped(Some(&["*.md", "specs/**"]), &[], None));
    assert!(takes(&schema, "docs/spec/SP-1.md"));
    assert!(takes(&schema, "docs/spec/specs/a.md"));
    assert!(takes(&schema, "docs/spec/specs/deep/a.md"));
    // `*.md` is the directory itself, and `specs/**` is under `specs`.
    assert!(!takes(&schema, "docs/spec/other/a.md"));
    assert!(!takes(&schema, "docs/spec/a/b.md"));
}

#[test]
fn an_exclude_takes_files_out_of_the_range_in_nested_directories_too() {
    let schema = schema_of(scoped(None, &["archive/**", "**/draft-*.md"], None));
    assert!(takes(&schema, "docs/spec/SP-1.md"));
    assert!(takes(&schema, "docs/spec/a/b/c.md"));
    assert!(!takes(&schema, "docs/spec/archive/a.md"));
    assert!(!takes(&schema, "docs/spec/archive/deep/a.md"));
    assert!(!takes(&schema, "docs/spec/draft-1.md"));
    assert!(!takes(&schema, "docs/spec/a/draft-2.md"));
    // `archive` only at the top.
    assert!(takes(&schema, "docs/spec/a/archive/b.md"));
}

#[test]
fn an_exclude_wins_over_an_include() {
    let schema = schema_of(scoped(Some(&["**/*.md"]), &["**/old-*.md"], None));
    assert!(takes(&schema, "docs/spec/new-1.md"));
    assert!(!takes(&schema, "docs/spec/a/old-1.md"));
}

#[test]
fn the_range_matches_case_as_written() {
    let schema = schema_of(scoped(Some(&["Specs/**", "*.md"]), &[], None));
    assert!(takes(&schema, "docs/spec/Specs/a.md"));
    assert!(!takes(&schema, "docs/spec/specs/a.md"));
}

#[test]
fn the_scope_is_kept_as_declared() {
    let schema = schema_of(scoped(Some(&["*.md"]), &["x/**"], Some("skip")));
    let scope = schema.book("spec").unwrap().scope();
    assert_eq!(
        scope
            .include()
            .unwrap()
            .iter()
            .map(Pattern::as_str)
            .collect::<Vec<_>>(),
        ["*.md"]
    );
    assert_eq!(scope.exclude().unwrap()[0].as_str(), "x/**");
    assert_eq!(
        scope.without_front_matter_declared(),
        Some(WithoutFrontMatter::Skip)
    );
    assert_eq!(scope.without_front_matter(), WithoutFrontMatter::Skip);
    let plain = schema_of(ScopeDecl::default());
    let scope = plain.book("spec").unwrap().scope();
    assert!(scope.include().is_none());
    assert!(scope.exclude().is_none());
    assert!(scope.without_front_matter_declared().is_none());
    assert_eq!(scope.without_front_matter(), WithoutFrontMatter::Invalid);
}

#[test]
fn a_pattern_that_is_not_a_glob_is_refused_naming_the_book_and_the_pattern() {
    let found = problems(scoped(Some(&["**/*.md"]), &["a/[b"], None));
    let [
        SchemaProblem::BadScopePattern {
            book,
            list,
            pattern,
            why: PatternProblem::Reserved { ch: '[' },
        },
    ] = found.as_slice()
    else {
        panic!("{found:?}");
    };
    assert_eq!(
        (book.as_str(), *list, pattern.as_str()),
        ("spec", ScopeList::Exclude, "a/[b")
    );
    let said = found[0].to_string();
    assert!(
        said.contains("spec") && said.contains("a/[b") && said.contains("exclude"),
        "{said}"
    );
}

#[test]
fn a_pattern_that_leaves_the_directory_is_refused() {
    for (pattern, why) in [
        ("/abs/*.md", PatternProblem::Absolute),
        ("../x/*.md", PatternProblem::ParentSegment),
        ("a/../../x.md", PatternProblem::ParentSegment),
        ("a//b.md", PatternProblem::EmptySegment),
        ("./a.md", PatternProblem::EmptySegment),
        ("", PatternProblem::Empty),
    ] {
        for exclude in [false, true] {
            let scope = if exclude {
                scoped(None, &[pattern], None)
            } else {
                scoped(Some(&[pattern, "*.md"]), &[], None)
            };
            let found = problems(scope);
            assert!(
                matches!(
                    found.as_slice(),
                    [SchemaProblem::BadScopePattern { why: w, pattern: p, .. }]
                        if *w == why && p == pattern
                ),
                "{pattern:?}: {found:?}"
            );
        }
    }
}

#[test]
fn a_negation_and_a_backslash_are_refused() {
    for (pattern, why) in [
        ("!archive/**", PatternProblem::Negated),
        ("a\\b.md", PatternProblem::Backslash),
        ("\\*.md", PatternProblem::Backslash),
    ] {
        let found = problems(scoped(None, &[pattern], None));
        assert!(
            matches!(
                found.as_slice(),
                [SchemaProblem::BadScopePattern { why: w, list: ScopeList::Exclude, .. }]
                    if *w == why
            ),
            "{pattern:?}: {found:?}"
        );
    }
    // A `!` that does not lead is a character of a name.
    assert!(takes(
        &schema_of(scoped(Some(&["a!.md"]), &[], None)),
        "docs/spec/a!.md"
    ));
}

#[test]
fn classes_and_alternatives_are_not_in_the_grammar() {
    for (pattern, ch) in [
        ("[ab].md", '['),
        ("a]b.md", ']'),
        ("{a,b}.md", '{'),
        ("a}.md", '}'),
        ("x/[!a]/**", '['),
    ] {
        let found = problems(scoped(Some(&[pattern]), &[], None));
        assert!(
            matches!(
                found.as_slice(),
                [SchemaProblem::BadScopePattern { why: PatternProblem::Reserved { ch: c }, .. }]
                    if *c == ch
            ),
            "{pattern:?}: {found:?}"
        );
    }
}

#[test]
fn a_double_star_is_a_whole_segment() {
    for pattern in ["sub**/*.md", "**sub/*.md", "a/b**", "**.md", "x/***"] {
        let found = problems(scoped(None, &[pattern], None));
        assert!(
            matches!(
                found.as_slice(),
                [SchemaProblem::BadScopePattern {
                    why: PatternProblem::SplitDoubleStar,
                    ..
                }]
            ),
            "{pattern:?}: {found:?}"
        );
    }
    // As a whole segment, anywhere.
    for pattern in ["**", "**/*.md", "a/**/b/*.md", "**/old/**"] {
        schema_of(scoped(None, &[pattern], None));
    }
}

#[test]
fn a_pattern_that_names_no_markdown_file_is_refused_with_the_way_to_write_it() {
    for pattern in ["archive", "a/b", "notes.txt", "x/README"] {
        let found = problems(scoped(None, &[pattern], None));
        let [SchemaProblem::BadScopePattern { why, .. }] = found.as_slice() else {
            panic!("{pattern:?}: {found:?}");
        };
        assert_eq!(
            why,
            &PatternProblem::NoFile {
                text: pattern.to_owned()
            }
        );
        assert!(
            found[0]
                .to_string()
                .contains(&format!("write `{pattern}/**`")),
            "{}",
            found[0]
        );
    }
    // A last segment with a wildcard or a `.md` name takes files.
    for pattern in ["archive/*", "README.md", "a/?.md", "x/**"] {
        schema_of(scoped(Some(&[pattern]), &[], None));
    }
}

#[test]
fn a_question_mark_and_a_star_stop_at_a_slash() {
    let schema = schema_of(scoped(Some(&["a?b.md", "x*.md"]), &[], None));
    assert!(takes(&schema, "docs/spec/acb.md"));
    assert!(!takes(&schema, "docs/spec/a/b.md"));
    assert!(takes(&schema, "docs/spec/xyz.md"));
    assert!(!takes(&schema, "docs/spec/x/y.md"));
}

#[test]
fn unicode_is_matched_byte_for_byte() {
    // `é` as one code point (NFC) and as `e` with a combining accent (NFD).
    let schema = schema_of(scoped(Some(&["caf\u{e9}.md"]), &[], None));
    assert!(takes(&schema, "docs/spec/caf\u{e9}.md"));
    assert!(!takes(&schema, "docs/spec/cafe\u{301}.md"));
}

#[test]
fn an_exclude_of_a_whole_directory_lets_a_listing_leave_it_out() {
    let range = |exclude: &[&str]| schema_of(scoped(None, exclude, None)).ranges().remove(0);
    let r = range(&["archive/**"]);
    assert!(r.leaves_out_dir("archive"));
    assert!(r.leaves_out_dir("archive/deep"));
    assert!(!r.leaves_out_dir("archives"));
    assert!(!r.leaves_out_dir("a/archive"));
    let r = range(&["**/drafts/**"]);
    assert!(r.leaves_out_dir("drafts"));
    assert!(r.leaves_out_dir("a/b/drafts/c"));
    assert!(!r.leaves_out_dir("a/b"));
    assert!(range(&["**"]).leaves_out_dir("anything"));
    // An exclude of files, or none: every directory is looked in.
    assert!(!range(&["archive/*.md"]).leaves_out_dir("archive"));
    assert!(!range(&[]).leaves_out_dir("archive"));
}

#[test]
fn a_range_that_does_not_take_the_key_paths_is_sound() {
    // The documents stay where they are; `new` refuses a key, key by key.
    schema_of(scoped(Some(&["only/**"]), &[], None));
    schema_of(scoped(None, &["SP-*.md"], None));
}

#[test]
fn a_book_of_slugs_is_not_held_to_one_key_path() {
    let slug = BookSchemaDecl {
        key: KeyPatternDecl::Slug,
        ..book_with(
            "note",
            "notes",
            "N",
            scoped(Some(&["topics/**"]), &[], None),
        )
    };
    Schema::validate(vec![slug], &spaces()).unwrap();
}

#[test]
fn an_empty_include_and_an_unknown_word_are_refused() {
    let found = problems(scoped(Some(&[]), &[], None));
    assert!(
        matches!(found.as_slice(), [SchemaProblem::EmptyInclude { book }] if book == "spec"),
        "{found:?}"
    );
    let found = problems(scoped(None, &[], Some("ignore")));
    assert!(
        matches!(
            found.as_slice(),
            [SchemaProblem::BadWithoutFrontMatter { word, .. }] if word == "ignore"
        ),
        "{found:?}"
    );
    assert!(found[0].to_string().contains("\"invalid\" or \"skip\""));
}

#[test]
fn every_problem_of_a_range_is_returned_at_once() {
    let found = problems(scoped(Some(&["/x"]), &["[", "../y"], Some("no")));
    assert_eq!(found.len(), 4, "{found:?}");
}

// ---- what a file with no front matter becomes ----

fn no_front_matter(file: &str) -> DocReading {
    DocReading::Unparsed {
        origin: path(file),
        id: ReadId::Unreadable,
        why: RawProblem::NoFrontMatter,
    }
}

fn unparsed(file: &str, why: RawProblem) -> DocReading {
    DocReading::Unparsed {
        origin: path(file),
        id: ReadId::Unreadable,
        why,
    }
}

fn built(scope: ScopeDecl, readings: Vec<DocReading>) -> Shelf {
    Shelf::build(&schema_of(scope), readings, &ArtifactReadings::new())
}

#[test]
fn a_file_with_no_front_matter_is_invalid_unless_its_book_skips_such_files() {
    let readings = || vec![no_front_matter("docs/spec/a.md")];
    let default = built(ScopeDecl::default(), readings());
    assert_eq!(default.unreadable().len(), 1);
    assert!(default.skipped().is_empty());

    let invalid = built(scoped(None, &[], Some("invalid")), readings());
    assert_eq!(invalid.unreadable().len(), 1);

    let skipping = built(scoped(None, &[], Some("skip")), readings());
    assert!(skipping.unreadable().is_empty());
    assert_eq!(skipping.tally_by_book()["spec"].invalid, 0);
    let [skipped] = skipping.skipped() else {
        panic!("{:?}", skipping.skipped());
    };
    assert_eq!(skipped.origin().as_str(), "docs/spec/a.md");
    assert_eq!(skipped.book().as_str(), "spec");
    assert_eq!(skipped.why(), SkipWhy::NoFrontMatter);
    assert_eq!(skipped.why().word(), "no_front_matter");
}

#[test]
fn a_front_matter_that_is_broken_is_invalid_even_when_the_book_skips() {
    let skip = || scoped(None, &[], Some("skip"));
    for why in [
        RawProblem::Unclosed,
        RawProblem::BadOpening,
        RawProblem::NotAMapping,
        RawProblem::Syntax { line: Some(2) },
    ] {
        let shelf = built(skip(), vec![unparsed("docs/spec/a.md", why.clone())]);
        assert!(shelf.skipped().is_empty(), "{why:?}");
        assert_eq!(shelf.unreadable().len(), 1, "{why:?}");
    }
}

#[test]
fn a_skipped_file_is_no_document_and_does_not_hide_the_real_ones() {
    let real = DocReading::Parsed {
        origin: path("docs/spec/SP-1.md"),
        entries: entries(&[("id", text(&docid(1).to_string())), ("key", text("SP-1"))]),
        body: String::new(),
    };
    let shelf = built(
        scoped(None, &[], Some("skip")),
        vec![
            no_front_matter("docs/spec/a.md"),
            real,
            no_front_matter("docs/spec/b/c.md"),
        ],
    );
    assert_eq!(shelf.docs().len(), 1);
    assert!(shelf.invalid().is_empty() && shelf.unreadable().is_empty());
    assert_eq!(shelf.skipped().len(), 2);
    let tally = shelf.tally_by_book()["spec"];
    assert_eq!((tally.valid, tally.invalid), (1, 0));
}

#[test]
fn a_file_out_of_the_range_or_under_no_book_is_never_a_skip() {
    let shelf = built(
        scoped(None, &["archive/**"], Some("skip")),
        vec![
            // Out of the range, but handed to the build anyway: not this book's.
            no_front_matter("docs/spec/archive/a.md"),
            no_front_matter("elsewhere/a.md"),
        ],
    );
    assert!(shelf.skipped().is_empty());
    assert_eq!(shelf.unreadable().len(), 2);
}

#[test]
fn a_book_that_skips_does_not_make_another_book_skip() {
    let a = book_with("spec", "docs/spec", "SP", scoped(None, &[], Some("skip")));
    let b = book_with("note", "docs/note", "NT", ScopeDecl::default());
    let schema = Schema::validate(vec![a, b], &spaces()).unwrap();
    let shelf = Shelf::build(
        &schema,
        vec![
            no_front_matter("docs/spec/a.md"),
            no_front_matter("docs/note/a.md"),
        ],
        &ArtifactReadings::new(),
    );
    assert_eq!(shelf.skipped().len(), 1);
    assert_eq!(shelf.unreadable().len(), 1);
    assert_eq!(shelf.unreadable()[0].origin().as_str(), "docs/note/a.md");
}

// ---- `new` ----

#[test]
fn a_new_document_outside_the_range_is_refused_naming_the_book_and_the_path() {
    let schema = Schema::validate(
        vec![BookSchemaDecl {
            key: KeyPatternDecl::Slug,
            ..book_with(
                "spec",
                "docs/spec",
                "SP",
                scoped(None, &["archive/**"], None),
            )
        }],
        &spaces(),
    )
    .unwrap();
    let shelf = Shelf::build(&schema, Vec::new(), &ArtifactReadings::new());
    let book = BookName::new("spec").unwrap();
    let accepted = |key: &str| {
        let key = shelf.claim_key(&book, key).unwrap();
        shelf
            .check_new(
                docid(9),
                &key,
                Default::default(),
                String::new(),
                &ArtifactReadings::new(),
            )
            .is_ok()
    };
    assert!(accepted("fine"));
    assert!(accepted("deep/er"));
    // The key step refuses it, before any identity is asked for.
    let Err(ClaimKeyProblem::OutOfRange(out)) = shelf.claim_key(&book, "archive/x") else {
        panic!("not refused");
    };
    assert_eq!(out.book.as_str(), "spec");
    assert_eq!(out.path.as_str(), "docs/spec/archive/x.md");
    let said = out.to_string();
    assert!(
        said.contains("`spec`")
            && said.contains("docs/spec/archive/x.md")
            && said.contains("include every `.md` file")
            && said.contains("exclude `archive/**`"),
        "{said}"
    );
}

#[test]
fn the_next_key_outside_the_range_is_refused_naming_the_patterns() {
    let schema = schema_of(scoped(Some(&["notes/**"]), &["notes/old/**"], None));
    let shelf = Shelf::build(&schema, Vec::new(), &ArtifactReadings::new());
    let book = BookName::new("spec").unwrap();
    let Err(NextKeyProblem::OutOfRange(out)) = shelf.next_key(&book) else {
        panic!("not refused");
    };
    assert_eq!(out.path.as_str(), "docs/spec/SP-1.md");
    assert_eq!(
        out.to_string(),
        "the book `spec` does not take `docs/spec/SP-1.md`, the file of the key `SP-1`: \
         include `notes/**`; exclude `notes/old/**`"
    );
    assert!(matches!(
        shelf.claim_key(&book, "SP-7"),
        Err(ClaimKeyProblem::OutOfRange(_))
    ));
    // In range, the key is given.
    let open = schema_of(scoped(Some(&["SP-*.md"]), &[], None));
    let shelf = Shelf::build(&open, Vec::new(), &ArtifactReadings::new());
    assert!(shelf.next_key(&book).is_ok());
}
