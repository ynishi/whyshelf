//! Tests of the comparison an artifact pin keeps: taken from the link, written only
//! when it is not `bytes`, and read back strictly.

use crate::domain::canon::Canon;
use crate::domain::shelf::fixtures_tests::{
    approve, docid, fr, ledger, lines, schema, schema_of_lines, shelf_reading, sp_testing,
};
use crate::domain::shelf::schema::Schema;

use super::*;

/// The approvals of FR-1 and SP-2 (tested by `tests/a.rs`) under `schema`.
fn approval_of_spec(schema: &Schema) -> Record {
    let s = shelf_reading(
        schema,
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", b"// note\nassert!(x);\n")],
    );
    let first = approve(&s, &ledger(vec![]), 1, 1001, 10, None);
    approve(&s, &ledger(vec![first]), 2, 1002, 10, None)
}

fn pins_of(record: &Record) -> Vec<ArtifactPin> {
    let Record::Decision(d) = record else {
        panic!("a decision");
    };
    let DecisionVerb::Approve { artifacts, .. } = d.verb() else {
        panic!("an approval");
    };
    artifacts.iter().cloned().collect()
}

fn map_mut(canon: &mut Canon) -> &mut BTreeMap<String, Canon> {
    match canon {
        Canon::Map(map) => map,
        other => panic!("not a map: {other:?}"),
    }
}

/// The canonical form of the record, with the one artifact pin given to `change`.
fn with_pin(record: &Record, change: impl FnOnce(&mut BTreeMap<String, Canon>)) -> Canon {
    let mut canon = record.to_canon();
    let Some(Canon::List(pins)) = map_mut(&mut canon).get_mut("artifacts") else {
        panic!("an approval with artifacts");
    };
    assert_eq!(pins.len(), 1);
    change(map_mut(&mut pins[0]));
    canon
}

#[test]
fn a_pin_keeps_the_comparison_of_its_link_and_reads_back_with_it() {
    let record = approval_of_spec(&schema_of_lines(&["//"]));
    let pins = pins_of(&record);
    assert_eq!(pins[0].comparison(), &lines(&["//"]));
    let PinnedArtifact::Present { hash, .. } = pins[0].seen() else {
        panic!("the file is there");
    };
    assert_eq!(*hash, lines(&["//"]).fingerprint(b"assert!(x);\n"));

    let canon = record.to_canon();
    let back = Record::from_canon(&canon).unwrap();
    assert_eq!(back, record);
    assert_eq!(back.hash(), record.hash());
    let mut written = canon.clone();
    let Some(Canon::List(written_pins)) = map_mut(&mut written).get("artifacts").cloned() else {
        panic!("artifacts");
    };
    let mut pin = written_pins[0].clone();
    assert_eq!(
        map_mut(&mut pin).get("compare"),
        Some(&Canon::map([
            ("kind", Canon::str("lines")),
            ("ignore", Canon::list([Canon::str("//")])),
        ]))
    );
}

#[test]
fn a_pin_of_bytes_has_the_form_and_the_record_hash_it_had_before_comparisons() {
    let record = approval_of_spec(&schema());
    assert_eq!(pins_of(&record)[0].comparison(), &Comparison::Bytes);
    let canon = record.to_canon();
    let mut keys = Vec::new();
    let _ = with_pin(&record, |pin| {
        keys = pin.keys().cloned().collect();
    });
    // No `compare`, as in a pin of the first version (which named its file by
    // `path`; a pin now names its space and its name).
    assert_eq!(
        keys,
        ["hash", "link", "name", "space", "state"].map(String::from)
    );
    // A record written by hand the old way, with the same pin and no `compare`, is
    // this record, with its hash.
    let old = Record::from_canon(&with_pin(&record, |pin| {
        pin.remove("compare");
    }))
    .unwrap();
    assert_eq!(old.to_canon(), canon);
    assert_eq!(old.hash(), record.hash());
    assert_eq!(Record::from_canon(&canon).unwrap().hash(), record.hash());
}

#[test]
fn a_pin_that_writes_bytes_or_no_comparison_is_refused() {
    let record = approval_of_spec(&schema_of_lines(&["//"]));
    let set = |compare: Canon| {
        Record::from_canon(&with_pin(&record, |pin| {
            pin.insert("compare".to_owned(), compare);
        }))
    };
    let bytes = set(Canon::map([
        ("kind", Canon::str("bytes")),
        ("ignore", Canon::list([])),
    ]));
    assert!(
        matches!(&bytes, Err(RecordProblem::WrongType { key, .. }) if key == "compare"),
        "{bytes:?}"
    );
    let unknown = set(Canon::map([
        ("kind", Canon::str("words")),
        ("ignore", Canon::list([])),
    ]));
    assert!(
        matches!(
            &unknown,
            Err(RecordProblem::BadComparison(
                ComparisonProblem::UnknownKind { .. }
            ))
        ),
        "{unknown:?}"
    );
    let prefix = set(Canon::map([
        ("kind", Canon::str("lines")),
        ("ignore", Canon::list([Canon::str("a b")])),
    ]));
    assert!(
        matches!(
            &prefix,
            Err(RecordProblem::BadComparison(
                ComparisonProblem::BadPrefix { .. }
            ))
        ),
        "{prefix:?}"
    );
    let extra = set(Canon::map([
        ("kind", Canon::str("lines")),
        ("ignore", Canon::list([])),
        ("case", Canon::str("fold")),
    ]));
    assert!(
        matches!(&extra, Err(RecordProblem::ExtraKey { key }) if key == "case"),
        "{extra:?}"
    );
    assert!(set(Canon::str("lines")).is_err());
    // The prefixes are written sorted and once each, as the record writes them, so a
    // record has one form: another order, or a prefix twice, is refused.
    let prefixes = |ignore: &[&str]| {
        set(Canon::map([
            ("kind", Canon::str("lines")),
            ("ignore", Canon::list(ignore.iter().map(|p| Canon::str(*p)))),
        ]))
    };
    for ignore in [&["//", "#"][..], &["//", "//"]] {
        let result = prefixes(ignore);
        assert!(
            matches!(&result, Err(RecordProblem::WrongType { key, .. }) if key == "ignore"),
            "{ignore:?}: {result:?}"
        );
    }
    assert!(prefixes(&["#", "//"]).is_ok());
    assert!(prefixes(&["//"]).is_ok());
    // The subject is still the one approved.
    assert!(matches!(&record, Record::Decision(d) if d.subject() == docid(2)));
}
