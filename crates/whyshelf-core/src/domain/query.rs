//! The `find` query language: a parser into a syntax tree.
//!
//! A query is white-space separated terms, all of which must match (a logical
//! AND). A term is one of:
//!
//! | term | meaning |
//! |---|---|
//! | `tag:<tag>` | the entity has the tag (`tag:area:auth`; everything after the first `:` is the tag) |
//! | `kind:<kind>` | `finding`, `spec`, `design` or `item` |
//! | `state:<state>` | one of the state values: `invalid`, `draft`, `approved`, `modified` (approval axis), `ok`, `stale`, `blocked` (upstream axis) or `unresolved` (partial: something it stands on cannot be read here) |
//! | `owner:<name>` | the owner |
//! | `grounds:<ref>` | entities that take this Finding as a ground |
//! | `upstream:<ref>` | entities whose upstream is this one |
//! | `orphan:true` / `orphan:false` | referenced from nowhere / referenced |
//! | a bare word or a `"quoted text"` | full-text search of the body |
//!
//! A value may be quoted to hold white space (`owner:"alice smith"`); inside quotes
//! `\"` is a quote and `\\` a backslash. The query `owner:alice "request timeout"`
//! asks for entities owned by `alice` whose text contains `request timeout`.
//!
//! The syntax tree is what the application evaluates against its index; the
//! language knows no SQL and the index is not part of any contract. An empty query
//! has no terms and matches everything.

use std::iter::Peekable;
use std::str::Chars;

use super::actor::Actor;
use super::error::QueryError;
use super::event::EntityKind;
use super::nonempty::NonBlank;
use super::reference::LocalRef;
use super::state::{Approval, EntityState, Upstream};
use super::tag::{Tag, query_tag};

/// One value of the state axes, as `state:` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateFilter {
    /// Approval axis: invalid.
    Invalid,
    /// Approval axis: draft.
    Draft,
    /// Approval axis: approved.
    Approved,
    /// Approval axis: modified.
    Modified,
    /// Upstream axis: ok.
    Ok,
    /// Upstream axis: stale.
    Stale,
    /// Upstream axis: blocked.
    Blocked,
    /// Completeness: partial.
    Unresolved,
}

impl StateFilter {
    fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "invalid" => Some(Self::Invalid),
            "draft" => Some(Self::Draft),
            "approved" => Some(Self::Approved),
            "modified" => Some(Self::Modified),
            "ok" => Some(Self::Ok),
            "stale" => Some(Self::Stale),
            "blocked" => Some(Self::Blocked),
            "unresolved" => Some(Self::Unresolved),
            _ => None,
        }
    }

    /// Whether a state has this value on the axis the value belongs to.
    pub fn matches(self, state: &EntityState) -> bool {
        match self {
            Self::Invalid => matches!(state.approval(), Approval::Invalid(_)),
            Self::Draft => matches!(state.approval(), Approval::Draft),
            Self::Approved => matches!(state.approval(), Approval::Approved),
            Self::Modified => matches!(state.approval(), Approval::Modified { .. }),
            Self::Ok => matches!(state.upstream(), Upstream::Ok),
            Self::Stale => matches!(state.upstream(), Upstream::Stale(_)),
            Self::Blocked => matches!(state.upstream(), Upstream::Blocked(_)),
            Self::Unresolved => state.completeness().is_partial(),
        }
    }
}

/// One term of a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    /// Has this tag.
    Tag(Tag),
    /// Is of this kind.
    Kind(EntityKind),
    /// Has this state value.
    State(StateFilter),
    /// Is owned by this person.
    Owner(Actor),
    /// Takes this entity as a ground.
    Grounds(LocalRef),
    /// Has this entity upstream.
    Upstream(LocalRef),
    /// Is (`true`) or is not (`false`) referenced from anywhere.
    Orphan(bool),
    /// Contains this text.
    Text(NonBlank),
}

/// A parsed query: every term must match.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    terms: Vec<Term>,
}

impl Query {
    /// The terms, in the order written.
    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    /// Whether there is no term, so everything matches.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Parse a query text.
    pub fn parse(input: &str) -> Result<Self, QueryError> {
        let mut chars = input.chars().peekable();
        let mut terms = Vec::new();
        loop {
            skip_white_space(&mut chars);
            let Some(&first) = chars.peek() else {
                break;
            };
            if first == '"' {
                terms.push(text_term(read_quoted(&mut chars)?)?);
                continue;
            }
            let mut word = String::new();
            let mut has_key = false;
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                chars.next();
                if c == ':' {
                    has_key = true;
                    break;
                }
                word.push(c);
            }
            if !has_key {
                terms.push(text_term(word)?);
                continue;
            }
            let value = if chars.peek() == Some(&'"') {
                read_quoted(&mut chars)?
            } else {
                let mut run = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() {
                        break;
                    }
                    run.push(c);
                    chars.next();
                }
                run
            };
            terms.push(keyed_term(&word, &value)?);
        }
        Ok(Self { terms })
    }
}

fn skip_white_space(chars: &mut Peekable<Chars<'_>>) {
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
}

fn read_quoted(chars: &mut Peekable<Chars<'_>>) -> Result<String, QueryError> {
    chars.next(); // the opening quote
    let mut out = String::new();
    loop {
        match chars.next() {
            None => return Err(QueryError::UnterminatedQuote),
            Some('"') => return Ok(out),
            Some('\\') => match chars.next() {
                None => return Err(QueryError::DanglingEscape),
                Some(c @ ('"' | '\\')) => out.push(c),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
            },
            Some(c) => out.push(c),
        }
    }
}

fn text_term(text: String) -> Result<Term, QueryError> {
    NonBlank::new(text)
        .map(Term::Text)
        .map_err(|_| QueryError::EmptyText)
}

fn keyed_term(key: &str, value: &str) -> Result<Term, QueryError> {
    if value.is_empty() {
        return Err(QueryError::EmptyValue {
            key: key.to_owned(),
        });
    }
    match key {
        "tag" => query_tag(value).map(Term::Tag),
        "kind" => match value.to_ascii_lowercase().as_str() {
            "finding" => Ok(Term::Kind(EntityKind::Finding)),
            "spec" => Ok(Term::Kind(EntityKind::Spec)),
            "design" => Ok(Term::Kind(EntityKind::Design)),
            "item" => Ok(Term::Kind(EntityKind::Item)),
            _ => Err(QueryError::BadKind {
                found: value.to_owned(),
            }),
        },
        "state" => StateFilter::parse(value)
            .map(Term::State)
            .ok_or_else(|| QueryError::BadState {
                found: value.to_owned(),
            }),
        "owner" => Actor::new(value)
            .map(Term::Owner)
            .map_err(QueryError::BadOwner),
        "grounds" => value
            .parse()
            .map(Term::Grounds)
            .map_err(|source| QueryError::BadRef {
                key: "grounds",
                source,
            }),
        "upstream" => value
            .parse()
            .map(Term::Upstream)
            .map_err(|source| QueryError::BadRef {
                key: "upstream",
                source,
            }),
        "orphan" => match value {
            "true" => Ok(Term::Orphan(true)),
            "false" => Ok(Term::Orphan(false)),
            other => Err(QueryError::BadBool {
                found: other.to_owned(),
            }),
        },
        other => Err(QueryError::UnknownKey {
            key: other.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::ValueError;
    use crate::domain::id::test_support::eid;
    use crate::domain::nonempty::NonEmptyVec;
    use crate::domain::state::{
        BlockReason, Completeness, InvalidReason, StaleReason, UnresolvedReason,
    };

    fn parse(q: &str) -> Vec<Term> {
        Query::parse(q).unwrap().terms().to_vec()
    }

    fn text(s: &str) -> Term {
        Term::Text(NonBlank::new(s).unwrap())
    }

    #[test]
    fn the_documented_examples_parse() {
        assert_eq!(
            parse("tag:area:auth kind:spec state:stale"),
            vec![
                Term::Tag("area:auth".parse().unwrap()),
                Term::Kind(EntityKind::Spec),
                Term::State(StateFilter::Stale),
            ]
        );
        assert_eq!(
            parse(r#"owner:alice "timeout""#),
            vec![Term::Owner(Actor::new("alice").unwrap()), text("timeout")]
        );
        assert_eq!(
            parse("grounds:finding/slow-login"),
            vec![Term::Grounds(LocalRef::Alias(
                "finding/slow-login".parse().unwrap()
            ))]
        );
        assert_eq!(
            parse("upstream:spec/cache-timeout"),
            vec![Term::Upstream(LocalRef::Alias(
                "spec/cache-timeout".parse().unwrap()
            ))]
        );
        assert_eq!(parse("orphan:true"), vec![Term::Orphan(true)]);
        assert_eq!(parse("orphan:false"), vec![Term::Orphan(false)]);
    }

    #[test]
    fn an_empty_query_has_no_terms() {
        for q in ["", "   ", "\t\n"] {
            let parsed = Query::parse(q).unwrap();
            assert!(parsed.is_empty());
        }
    }

    #[test]
    fn bare_words_are_text_and_terms_keep_their_order() {
        assert_eq!(parse("slow login"), vec![text("slow"), text("login")]);
        assert_eq!(
            parse("kind:item slow state:ok"),
            vec![
                Term::Kind(EntityKind::Item),
                text("slow"),
                Term::State(StateFilter::Ok)
            ]
        );
    }

    #[test]
    fn quoted_values_keep_white_space_and_escapes() {
        assert_eq!(
            parse(r#"owner:"alice smith""#),
            vec![Term::Owner(Actor::new("alice smith").unwrap())]
        );
        assert_eq!(parse(r#""a \"b\" c""#), vec![text(r#"a "b" c"#)]);
        assert_eq!(parse(r#""back\\slash""#), vec![text(r"back\slash")]);
        assert_eq!(
            parse(r#""keep \n literal""#),
            vec![text(r"keep \n literal")]
        );
    }

    #[test]
    fn a_tag_keeps_every_colon_after_the_key() {
        assert_eq!(parse("tag:wip"), vec![Term::Tag("wip".parse().unwrap())]);
        assert!(matches!(
            Query::parse("tag:a:b:c"),
            Err(QueryError::BadTag(ValueError::TagBadChar { ch: ':' }))
        ));
    }

    #[test]
    fn kinds_and_states_are_case_insensitive() {
        assert_eq!(parse("kind:Spec"), vec![Term::Kind(EntityKind::Spec)]);
        assert_eq!(parse("state:DRAFT"), vec![Term::State(StateFilter::Draft)]);
        for (name, filter) in [
            ("invalid", StateFilter::Invalid),
            ("draft", StateFilter::Draft),
            ("approved", StateFilter::Approved),
            ("modified", StateFilter::Modified),
            ("ok", StateFilter::Ok),
            ("stale", StateFilter::Stale),
            ("blocked", StateFilter::Blocked),
            ("unresolved", StateFilter::Unresolved),
        ] {
            assert_eq!(parse(&format!("state:{name}")), vec![Term::State(filter)]);
        }
        for (name, kind) in [
            ("finding", EntityKind::Finding),
            ("design", EntityKind::Design),
            ("item", EntityKind::Item),
        ] {
            assert_eq!(parse(&format!("kind:{name}")), vec![Term::Kind(kind)]);
        }
    }

    #[test]
    fn a_reference_value_can_be_an_identifier() {
        let id = eid(3);
        assert_eq!(
            parse(&format!("grounds:{id}")),
            vec![Term::Grounds(LocalRef::Id(id))]
        );
        assert_eq!(LocalRef::Id(id).as_id(), Some(id));
        assert_eq!(LocalRef::Alias("a".parse().unwrap()).as_id(), None);
    }

    #[test]
    fn each_malformed_query_has_its_own_error() {
        assert_eq!(
            Query::parse("colour:red"),
            Err(QueryError::UnknownKey {
                key: "colour".into()
            })
        );
        assert_eq!(
            Query::parse("tag:"),
            Err(QueryError::EmptyValue { key: "tag".into() })
        );
        assert_eq!(Query::parse(r#""open"#), Err(QueryError::UnterminatedQuote));
        assert_eq!(Query::parse(r#""open\"#), Err(QueryError::DanglingEscape));
        assert_eq!(Query::parse(r#""""#), Err(QueryError::EmptyText));
        assert_eq!(
            Query::parse("kind:epic"),
            Err(QueryError::BadKind {
                found: "epic".into()
            })
        );
        assert_eq!(
            Query::parse("state:green"),
            Err(QueryError::BadState {
                found: "green".into()
            })
        );
        assert_eq!(
            Query::parse("orphan:maybe"),
            Err(QueryError::BadBool {
                found: "maybe".into()
            })
        );
        assert!(matches!(
            Query::parse("owner:\"\u{1}\""),
            Err(QueryError::BadOwner(_))
        ));
        assert!(matches!(
            Query::parse("grounds:Not Valid"),
            Err(QueryError::BadRef { key: "grounds", .. })
        ));
        assert!(matches!(
            Query::parse("upstream:Not"),
            Err(QueryError::BadRef {
                key: "upstream",
                ..
            })
        ));
    }

    #[test]
    fn state_filters_read_the_axis_they_name() {
        let invalid = EntityState::new(
            Approval::Invalid(NonEmptyVec::new(
                InvalidReason::DanglingRef { upstream: eid(1) },
                vec![],
            )),
            Upstream::Ok,
            Completeness::Complete,
        );
        let draft_blocked = EntityState::new(
            Approval::Draft,
            Upstream::Blocked(NonEmptyVec::new(
                BlockReason::UpstreamUnapproved { upstream: eid(1) },
                vec![],
            )),
            Completeness::Complete,
        );
        let approved_stale = EntityState::new(
            Approval::Approved,
            Upstream::Stale(NonEmptyVec::new(
                StaleReason::UpstreamStale { upstream: eid(1) },
                vec![],
            )),
            Completeness::Complete,
        );
        let modified = EntityState::new(
            Approval::Modified {
                approved: crate::domain::hash::ContentHash::raw(b"x"),
            },
            Upstream::Ok,
            Completeness::Complete,
        );
        let draft_partial = EntityState::new(
            Approval::Draft,
            Upstream::Ok,
            Completeness::from_reasons(
                [UnresolvedReason::new(eid(2), eid(2))]
                    .into_iter()
                    .collect(),
            ),
        );
        let table = [
            (&invalid, vec![StateFilter::Invalid, StateFilter::Ok]),
            (
                &draft_blocked,
                vec![StateFilter::Draft, StateFilter::Blocked],
            ),
            (
                &approved_stale,
                vec![StateFilter::Approved, StateFilter::Stale],
            ),
            (&modified, vec![StateFilter::Modified, StateFilter::Ok]),
            (
                &draft_partial,
                vec![StateFilter::Draft, StateFilter::Ok, StateFilter::Unresolved],
            ),
        ];
        let all = [
            StateFilter::Invalid,
            StateFilter::Draft,
            StateFilter::Approved,
            StateFilter::Modified,
            StateFilter::Ok,
            StateFilter::Stale,
            StateFilter::Blocked,
            StateFilter::Unresolved,
        ];
        for (state, expected) in table {
            for filter in all {
                assert_eq!(
                    filter.matches(state),
                    expected.contains(&filter),
                    "{filter:?}"
                );
            }
        }
    }
}
