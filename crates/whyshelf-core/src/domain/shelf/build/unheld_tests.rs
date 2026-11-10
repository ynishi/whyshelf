//! Tests of how a value a front matter does not hold is told: each kind has its own
//! words, and the usual causes their way out.

use super::{RawProblem, UnheldItem, UnheldKind};

const KINDS: [UnheldKind; 11] = [
    UnheldKind::Null,
    UnheldKind::Float,
    UnheldKind::TooLarge,
    UnheldKind::Mapping,
    UnheldKind::Tagged,
    UnheldKind::Item(UnheldItem::Null),
    UnheldKind::Item(UnheldItem::Bool),
    UnheldKind::Item(UnheldItem::Number),
    UnheldKind::Item(UnheldItem::Mapping),
    UnheldKind::Item(UnheldItem::List),
    UnheldKind::Item(UnheldItem::Tagged),
];

fn message(kind: UnheldKind) -> String {
    RawProblem::UnsupportedValue {
        key: "v".to_owned(),
        kind,
    }
    .to_string()
}

#[test]
fn each_kind_has_words_of_its_own() {
    let words: Vec<String> = KINDS.iter().map(ToString::to_string).collect();
    for (i, a) in words.iter().enumerate() {
        assert!(!a.is_empty());
        for b in &words[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn the_message_names_the_key_and_the_kind_and_says_front_matter_once() {
    for kind in KINDS {
        let text = message(kind);
        assert!(text.starts_with("the value of `v` is "), "{text}");
        assert!(text.contains(&kind.to_string()), "{text}");
        assert_eq!(text.matches("front matter").count(), 1, "{text}");
    }
}

#[test]
fn the_usual_causes_say_how_to_write_the_value() {
    let hint = |kind: UnheldKind| kind.hint();
    assert!(hint(UnheldKind::Item(UnheldItem::Mapping)).contains("put it in quotes"));
    assert!(hint(UnheldKind::Item(UnheldItem::Bool)).contains("quotes"));
    assert!(hint(UnheldKind::Item(UnheldItem::Number)).contains("quotes"));
    assert!(hint(UnheldKind::Item(UnheldItem::Null)).contains("take it out"));
    assert!(hint(UnheldKind::Float).contains("digits only"));
    assert!(hint(UnheldKind::TooLarge).contains("quotes"));
    assert!(hint(UnheldKind::Null).contains("leave the key out"));
    assert!(hint(UnheldKind::Mapping).contains("flat"));
    assert_eq!(hint(UnheldKind::Tagged), "");
}
