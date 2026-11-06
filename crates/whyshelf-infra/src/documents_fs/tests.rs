use super::*;

use std::path::Path;

use whyshelf_core::domain::corpus::Corpus;

fn id(n: u128) -> String {
    format!("0192f3a4-5b6c-7d8e-8f90-{n:012x}")
}

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

fn finding(id: &str, alias: Option<&str>) -> String {
    let alias = alias.map_or(String::new(), |a| format!("alias: {a}\n"));
    format!(
        "---\nid: {id}\n{alias}finding_type: usage_observation\ntitle: \"Slow login\"\nevidence: measured\ncontext: \"cold start\"\nreproducibility: reproducible\n---\n\nLogin takes 8 s.\n"
    )
}

fn spec(id: &str, grounds: &[&str]) -> String {
    let grounds: String = grounds.iter().map(|g| format!("  - {g}\n")).collect();
    format!(
        "---\nid: {id}\ngrounds:\n{grounds}accept:\n  pins:\n    - id: PIN-A\n      contract: c\n  hyp:\n    - id: HYP-A\n      metric: m\n      target: t\n---\n\n## Contract\nA session is reused.\n"
    )
}

fn design(id: &str, spec: &str) -> String {
    format!(
        "---\nid: {id}\nspec: {spec}\ncrux:\n  question: q\n  options:\n    - id: opt-a\n      summary: s\n      chosen: true\n      reason: r\n    - id: opt-b\n      summary: s\n      rejected: r\nitems:\n  - id: {item}\n    number: 1\n    summary: s\n    pins: [PIN-A]\n---\n\n## Design Notes\nx\n",
        item = id.replace("8f90", "8f91")
    )
}

async fn put(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(path, text).await.unwrap();
}

fn roots(public: &Path, private: Option<&Path>) -> FsDocuments {
    let root = |dir: &Path| DocumentRoot::new(dir.to_path_buf(), vec![rel("docs")]);
    FsDocuments::new(root(public), private.map(root))
}

fn reading<'a>(documents: &'a Documents, path: &str) -> &'a Reading {
    documents
        .files
        .iter()
        .find(|(facts, _)| facts.location().path().as_str() == path)
        .map(|(_, raw)| raw.reading())
        .unwrap_or_else(|| panic!("{path} is not listed"))
}

fn reasons_of(reading: &Reading) -> Vec<UnparsedReason> {
    match reading {
        Reading::Unparsed { reasons, .. } => reasons.iter().cloned().collect(),
        other => panic!("not unparsed: {other:?}"),
    }
}

#[tokio::test]
async fn a_file_is_listed_with_its_place_its_bytes_hash_and_its_normalized_text() {
    let public = tempfile::tempdir().unwrap();
    let text = finding(&id(1), None);
    let crlf = text.replace('\n', "\r\n").replace("8 s.", "8 s.   ");
    put(public.path(), "docs/a.md", &text).await;
    put(public.path(), "docs/b.md", &crlf).await;

    let documents = roots(public.path(), None).read_all().await.unwrap();
    let [(a_facts, a_raw), (b_facts, b_raw)] = documents.files.as_slice() else {
        panic!("two files, got {}", documents.files.len());
    };
    assert_eq!(a_facts.location().path().as_str(), "docs/a.md");
    assert_eq!(a_facts.location().visibility(), Visibility::Public);
    assert!(
        matches!(a_facts.location().git(), Knowable::Unknown),
        "Git is not asked here"
    );
    assert!(matches!(a_raw.reading(), Reading::Document(_)));
    assert_eq!(a_facts.bytes(), BytesHash::of(text.as_bytes()));

    // The same document with CRLF line ends and trailing spaces: one text, and two
    // files that are not copies of each other.
    assert_eq!(a_raw.text(), b_raw.text());
    assert_eq!(b_facts.bytes(), BytesHash::of(crlf.as_bytes()));
    assert_ne!(a_facts.bytes(), b_facts.bytes());
    assert_ne!(
        a_raw.text().text_hash(),
        whyshelf_core::domain::hash::TextHash::of(&crlf),
        "the text hash is of the normalized text, not of the bytes"
    );
}

#[tokio::test]
async fn both_roots_are_read_and_the_documents_build_a_corpus() {
    let public = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    put(
        public.path(),
        "docs/a.md",
        &finding(&id(1), Some("slow-login")),
    )
    .await;
    put(
        private.path(),
        "docs/p.md",
        &finding(&id(2), Some("secret-note")),
    )
    .await;

    let source = roots(public.path(), Some(private.path()));
    let files = source.list().await.unwrap();
    let roots_seen: Vec<_> = files
        .iter()
        .map(|(facts, _)| {
            (
                facts.location().visibility(),
                facts.location().path().as_str().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        roots_seen,
        [
            (Visibility::Public, "docs/a.md".to_owned()),
            (Visibility::Private, "docs/p.md".to_owned())
        ]
    );
    let corpus = Corpus::build(
        files.into_iter().map(|(facts, raw)| {
            let (reading, _) = raw.into_parts();
            (facts, reading)
        }),
        &BTreeSet::new(),
    );
    assert_eq!(corpus.documents().len(), 2);
}

#[tokio::test]
async fn a_file_without_an_id_is_unidentified_and_a_bad_id_is_unparsed() {
    let public = tempfile::tempdir().unwrap();
    let no_id: String = finding(&id(1), None).replace(&format!("id: {}\n", id(1)), "");
    put(public.path(), "docs/none.md", &no_id).await;
    put(
        public.path(),
        "docs/bad.md",
        &finding(&id(1), None).replace(&id(1), "not-an-id"),
    )
    .await;
    put(
        public.path(),
        "docs/two.md",
        &finding(&id(1), None).replace(
            &format!("id: {}\n", id(1)),
            &format!("id: {}\nid: {}\n", id(1), id(2)),
        ),
    )
    .await;

    let documents = roots(public.path(), None).read_all().await.unwrap();
    assert!(matches!(
        reading(&documents, "docs/none.md"),
        Reading::Unidentified {
            kind: Some(DocKind::Finding)
        }
    ));
    for name in ["docs/bad.md", "docs/two.md"] {
        let reasons = reasons_of(reading(&documents, name));
        assert!(
            matches!(reasons.as_slice(), [UnparsedReason::InvalidField { field, .. }] if field == "id"),
            "{name}: {reasons:?}"
        );
    }
}

#[tokio::test]
async fn a_file_that_does_not_read_is_listed_with_the_reasons_and_the_id_it_shows() {
    let public = tempfile::tempdir().unwrap();
    let conflicted = finding(&id(1), None).replace(
        "Login takes 8 s.",
        "<<<<<<< HEAD\na\n=======\nb\n>>>>>>> topic",
    );
    put(public.path(), "docs/conflict.md", &conflicted).await;
    put(public.path(), "docs/plain.md", "just a note\n").await;
    tokio::fs::write(public.path().join("docs/latin.md"), [0x66u8, 0xff, 0x6f])
        .await
        .unwrap();

    let documents = roots(public.path(), None).read_all().await.unwrap();
    let Reading::Unparsed { id: seen, reasons } = reading(&documents, "docs/conflict.md") else {
        panic!("a conflicted file reads");
    };
    assert_eq!(seen, &Some(id(1).parse().unwrap()));
    assert_eq!(reasons.first(), &UnparsedReason::MergeConflictMarker);
    assert!(!reasons_of(reading(&documents, "docs/plain.md")).is_empty());
    let latin = reasons_of(reading(&documents, "docs/latin.md"));
    assert!(matches!(
        latin.as_slice(),
        [UnparsedReason::MalformedFrontmatter { .. }]
    ));
    assert_eq!(documents.files.len(), 3, "none is left out");
}

#[tokio::test]
async fn a_spec_and_a_design_are_built_with_the_identifiers_their_references_resolve_to() {
    let public = tempfile::tempdir().unwrap();
    put(
        public.path(),
        "docs/f.md",
        &finding(&id(1), Some("slow-login")),
    )
    .await;
    put(public.path(), "docs/s.md", &spec(&id(2), &["slow-login"])).await;
    put(public.path(), "docs/s2.md", &spec(&id(3), &[&id(1)])).await;
    put(public.path(), "docs/d.md", &design(&id(4), &id(2))).await;

    let documents = roots(public.path(), None).read_all().await.unwrap();
    let grounds = |path: &str| match reading(&documents, path) {
        Reading::Document(doc) => match doc.entity() {
            Entity::Spec(spec) => spec
                .grounds()
                .iter()
                .map(|g| g.entity().to_string())
                .collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    assert_eq!(grounds("docs/s.md"), [id(1)], "by alias");
    assert_eq!(grounds("docs/s2.md"), [id(1)], "by identifier");
    let Reading::Document(design) = reading(&documents, "docs/d.md") else {
        panic!("the design reads");
    };
    let Entity::Design(design) = design.entity() else {
        panic!("a design");
    };
    assert_eq!(design.spec().entity().to_string(), id(2));
    assert!(
        documents
            .written
            .values()
            .all(|w| w.dangling.is_empty() && w.unresolved.is_empty())
    );
}

#[tokio::test]
async fn a_reference_that_names_nothing_is_dangling_by_identifier_and_unparsed_by_alias() {
    let public = tempfile::tempdir().unwrap();
    put(public.path(), "docs/by-id.md", &spec(&id(2), &[&id(9)])).await;
    put(
        public.path(),
        "docs/by-alias.md",
        &spec(&id(3), &["no-such-alias"]),
    )
    .await;
    put(
        public.path(),
        "docs/other-kind.md",
        &spec(&id(5), &[&id(2)]),
    )
    .await;

    let documents = roots(public.path(), None).read_all().await.unwrap();
    // Built with the identifier it wrote; the evaluation judges it.
    assert!(matches!(
        reading(&documents, "docs/by-id.md"),
        Reading::Document(_)
    ));
    let wrote = &documents.written[&id(2).parse::<DocId>().unwrap()];
    assert_eq!(wrote.dangling, BTreeSet::from([id(9).parse().unwrap()]));
    // No identifier, so nothing to build with.
    let reasons = reasons_of(reading(&documents, "docs/by-alias.md"));
    assert!(matches!(
        reasons.as_slice(),
        [UnparsedReason::DanglingRef { .. }]
    ));
    // A Spec named where a Finding is wanted.
    let reasons = reasons_of(reading(&documents, "docs/other-kind.md"));
    assert!(matches!(
        reasons.as_slice(),
        [UnparsedReason::WrongKind { .. }]
    ));
}

#[tokio::test]
async fn a_private_reference_is_unresolved_without_a_private_root_and_dangling_with_one() {
    let public = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let marked = format!("private:{}", id(7));
    put(public.path(), "docs/s.md", &spec(&id(2), &[&marked])).await;
    let doc = id(2).parse::<DocId>().unwrap();

    // Not here, and no private root: nobody can say it is mistyped.
    let documents = roots(public.path(), None).read_all().await.unwrap();
    assert!(matches!(
        reading(&documents, "docs/s.md"),
        Reading::Document(_)
    ));
    assert_eq!(
        documents.written[&doc].unresolved,
        BTreeSet::from([id(7).parse().unwrap()])
    );

    // A private root that does not hold it: mistyped.
    let documents = roots(public.path(), Some(private.path()))
        .read_all()
        .await
        .unwrap();
    assert_eq!(
        documents.written[&doc].dangling,
        BTreeSet::from([id(7).parse().unwrap()])
    );

    // A private root that holds it: found, from whichever root the file is in.
    put(
        private.path(),
        "docs/p.md",
        &finding(&id(7), Some("hidden")),
    )
    .await;
    let documents = roots(public.path(), Some(private.path()))
        .read_all()
        .await
        .unwrap();
    assert_eq!(documents.written[&doc], Written::default());
}

#[tokio::test]
async fn a_public_file_does_not_see_a_private_document_by_alias_and_a_private_file_sees_both() {
    let public = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    put(
        private.path(),
        "docs/p.md",
        &finding(&id(1), Some("hidden")),
    )
    .await;
    put(public.path(), "docs/f.md", &finding(&id(2), Some("shown"))).await;
    put(public.path(), "docs/leak.md", &spec(&id(3), &["hidden"])).await;
    put(
        private.path(),
        "docs/s.md",
        &spec(&id(4), &["hidden", "shown"]),
    )
    .await;

    let documents = roots(public.path(), Some(private.path()))
        .read_all()
        .await
        .unwrap();
    let reasons = reasons_of(reading(&documents, "docs/leak.md"));
    assert!(
        matches!(reasons.as_slice(), [UnparsedReason::DanglingRef { .. }]),
        "{reasons:?}"
    );
    let Reading::Document(doc) = reading(&documents, "docs/s.md") else {
        panic!("the private spec reads");
    };
    let Entity::Spec(spec) = doc.entity() else {
        panic!("a spec");
    };
    assert_eq!(spec.grounds().len(), 2);
}

#[tokio::test]
async fn an_alias_held_by_two_documents_is_ambiguous() {
    let public = tempfile::tempdir().unwrap();
    put(public.path(), "docs/a.md", &finding(&id(1), Some("same"))).await;
    put(public.path(), "docs/b.md", &finding(&id(2), Some("same"))).await;
    put(public.path(), "docs/s.md", &spec(&id(3), &["same"])).await;
    let documents = roots(public.path(), None).read_all().await.unwrap();
    let reasons = reasons_of(reading(&documents, "docs/s.md"));
    assert!(matches!(
        reasons.as_slice(),
        [UnparsedReason::AmbiguousRef { .. }]
    ));
}

#[tokio::test]
async fn the_kind_is_the_key_then_the_directory_then_the_keys_of_the_file() {
    let public = tempfile::tempdir().unwrap();
    let dir_kind = |dir: &str, kind| {
        DocumentRoot::new(public.path().to_path_buf(), vec![rel("docs")])
            .with_kind_dir(rel(dir), kind)
    };
    let with_kind = finding(&id(1), None).replace("finding_type", "kind: finding\nfinding_type");
    put(public.path(), "docs/finding/a.md", &with_kind).await;
    put(public.path(), "docs/finding/b.md", &finding(&id(2), None)).await;
    put(
        public.path(),
        "docs/spec/c.md",
        &with_kind.replace(&id(1), &id(3)),
    )
    .await;

    let source = FsDocuments::new(
        dir_kind("docs/finding", DocKind::Finding).with_kind_dir(rel("docs/spec"), DocKind::Spec),
        None,
    );
    let documents = source.read_all().await.unwrap();
    assert!(matches!(
        reading(&documents, "docs/finding/a.md"),
        Reading::Document(_)
    ));
    assert!(matches!(
        reading(&documents, "docs/finding/b.md"),
        Reading::Document(_)
    ));
    // `kind: finding` under a directory that holds specs.
    let reasons = reasons_of(reading(&documents, "docs/spec/c.md"));
    assert!(
        matches!(reasons.as_slice(), [UnparsedReason::InvalidField { field, .. }] if field == "kind"),
        "{reasons:?}"
    );

    // A kind that is none.
    put(
        public.path(),
        "docs/finding/d.md",
        &finding(&id(4), None).replace("finding_type", "kind: poem\nfinding_type"),
    )
    .await;
    let documents = source.read_all().await.unwrap();
    let reasons = reasons_of(reading(&documents, "docs/finding/d.md"));
    assert!(matches!(
        reasons.as_slice(),
        [UnparsedReason::InvalidField { field, .. }] if field == "kind"
    ));
}

#[tokio::test]
async fn a_root_that_is_not_there_lists_nothing() {
    let nowhere = tempfile::tempdir().unwrap().path().join("gone");
    let documents = roots(&nowhere, Some(&nowhere)).read_all().await.unwrap();
    assert!(documents.files.is_empty());
}

#[tokio::test]
async fn a_revision_is_accepted_when_it_is_a_full_hash() {
    let public = tempfile::tempdir().unwrap();
    let measured = |revision: &str| {
        format!(
            "---\nid: {}\nfinding_type: measured_data\ntitle: t\nevidence: measured\nmeasured_at: \"2026-11-04T09:30:00Z\"\nrevision: \"{revision}\"\nenvironment: e\n---\nbody\n",
            id(1)
        )
    };
    put(public.path(), "docs/ok.md", &measured(&"A1".repeat(20))).await;
    put(public.path(), "docs/short.md", &measured("a1b2c3")).await;
    let documents = roots(public.path(), None).read_all().await.unwrap();
    assert!(matches!(
        reading(&documents, "docs/ok.md"),
        Reading::Document(_)
    ));
    assert!(matches!(
        reasons_of(reading(&documents, "docs/short.md")).as_slice(),
        [UnparsedReason::RevisionRejected { .. }]
    ));
}
