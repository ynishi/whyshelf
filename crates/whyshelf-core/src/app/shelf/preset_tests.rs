//! Tests of the use cases of the shelf over a declaration with a default preset: it
//! is the declaration that writes the values out, in the schema and in freshness.

use crate::domain::shelf::freshness::{Cause, Freshness, Standing};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::map::{MapTarget, ShelfMap};
use crate::domain::shelf::schema::{
    DefaultsDecl, LinkTargetDecl, PresetName, PresetOrigin, ShelfDecl,
};

use super::fake_tests::{FakeShelf, block_on, decls, fr, sp_tested};
use super::{JudgeRequest, Judgement, MapReport, ShelfApp};

fn r(text: &str) -> Reference {
    text.parse().unwrap()
}

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
        },
        reference: r(reference),
        actor: None,
    }
}

/// The books of the fake, with `spec.tests` naming neither a space nor a meaning,
/// and the default `file` (space `file`, `bytes`, `depends`: what the fake's
/// `tests` writes out).
fn with_default() -> ShelfDecl {
    let mut decl = ShelfDecl::of_books(decls());
    for link in decl
        .books
        .iter_mut()
        .flat_map(|book| book.links.iter_mut())
        .filter(|link| link.target == LinkTargetDecl::Artifact)
    {
        link.space = None;
        link.propagation = None;
    }
    decl.defaults = DefaultsDecl {
        artifact: Some("file".to_owned()),
    };
    decl
}

/// FR-1, and SP-2 on it, tested by `tests/a.rs`; declared as `written` says, or as
/// the fake writes it out when none.
fn project(written: Option<ShelfDecl>) -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
    ]);
    *fake.written.lock().unwrap() = written;
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    ShelfApp::new(fake)
}

fn mapped(app: &ShelfApp<FakeShelf>) -> ShelfMap {
    match block_on(app.map()).unwrap() {
        MapReport::Mapped(map) => map,
        MapReport::SchemaInvalid(problems) => panic!("a sound declaration: {problems:?}"),
    }
}

/// The map, with the preset each link came from taken out.
fn without_presets(mut map: ShelfMap) -> ShelfMap {
    for link in &mut map.links {
        if let MapTarget::Artifact { preset, .. } = &mut link.target {
            *preset = None;
        }
    }
    map
}

#[test]
fn a_shelf_declared_with_a_default_has_the_schema_of_one_that_writes_the_space() {
    let defaulted = mapped(&project(Some(with_default())));
    let written = mapped(&project(None));
    let tests = defaulted
        .links
        .iter()
        .find(|link| link.name.as_str() == "tests")
        .unwrap();
    assert!(matches!(
        &tests.target,
        MapTarget::Artifact { preset: Some(PresetOrigin::Default(name)), .. }
            if *name == PresetName::new("file").unwrap()
    ));
    assert_eq!(without_presets(defaulted), written);
}

#[test]
fn a_shelf_declared_with_a_default_has_the_freshness_of_one_that_writes_the_space() {
    let mut defaulted = project(Some(with_default()));
    let mut written = project(None);
    for app in [&mut defaulted, &mut written] {
        block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    }
    // The approvals pin the same artifacts, the same way: no preset is in a record.
    let pins = |app: &ShelfApp<FakeShelf>| format!("{:?}", app.ports().records()[1]);
    assert_eq!(pins(&defaulted), pins(&written));

    for app in [&defaulted, &written] {
        app.ports().put_file("tests/a.rs", b"assert!(faster);\n");
    }
    let rows: Vec<Standing> = [&defaulted, &written]
        .into_iter()
        .map(|app| block_on(app.get(&r("SP-2"))).unwrap().row.standing)
        .collect();
    assert_eq!(rows[0], rows[1]);
    let Standing::Approved(Freshness::Degraded(d)) = &rows[0] else {
        panic!("the approval no longer holds: {:?}", rows[0]);
    };
    assert!(
        d.causes()
            .iter()
            .any(|c| matches!(c, Cause::ArtifactChanged { .. }))
    );
}
