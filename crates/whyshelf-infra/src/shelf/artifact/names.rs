//! What the adapters share in answering a move of a prefix
//! ([`NamePrefixes`](whyshelf_core::app::shelf::NamePrefixes)): a name built under the
//! new prefix as a name of the space, and the one shape the two spaces of tests
//! (`rust-test`, `js-test`) answer in, each by its own grammar.

use std::collections::BTreeSet;

use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, PrefixMove, Renamed, SpacePrefixes, SpaceRenamed,
};

/// The name an adapter built under the new prefix, as the text of a name of its
/// space: [`Renamed::Under`] when it is one (the checks every name has: not empty, at
/// most [`NAME_MAX`](whyshelf_core::domain::shelf::artifact::NAME_MAX) bytes, no
/// control character), else [`Renamed::Unbuildable`] with why.
pub(super) fn built(text: &str) -> Result<Renamed, ValueError> {
    Ok(match ArtifactName::new(text) {
        Ok(name) => Renamed::Under(name),
        Err(problem) => Renamed::Unbuildable(NonBlank::new(format!(
            "the name built under the new prefix is no name: {problem}"
        ))?),
    })
}

/// A name under the prefix of which the adapter can build no name under the new one,
/// with why.
pub(super) fn unbuildable(why: String) -> Result<Renamed, ValueError> {
    Ok(Renamed::Unbuildable(NonBlank::new(why)?))
}

/// One part of a name of a space of tests (a segment, a title) as it was written, with
/// the value the grammar of the space reads it as. A prefix is compared on the values
/// (`r#type` and `type` are the one segment) and a name is built of the texts, so that
/// a part keeps the spelling it was written in: the one a person typed in `to`, and the
/// one a name already has after `from`. The boundaries are the ones the adapter's own
/// parse found; no text is searched for one.
#[derive(Debug, Clone)]
pub(super) struct Spelled<V> {
    pub(super) value: V,
    pub(super) text: String,
}

impl<V> Spelled<V> {
    /// The part `text` writes, which reads as `value`.
    pub(super) fn new(value: V, text: &str) -> Self {
        Self {
            value,
            text: text.to_owned(),
        }
    }
}

/// The parts of `items` after `prefix`, when `items` start with all of the prefix's
/// parts, compared on the values (not on how each is written).
pub(super) fn after<'a, 'p, V: PartialEq + 'p>(
    items: &'a [Spelled<V>],
    prefix: impl IntoIterator<Item = &'p Spelled<V>>,
) -> Option<&'a [Spelled<V>]> {
    let mut taken = 0;
    for part in prefix {
        if items.get(taken)?.value != part.value {
            return None;
        }
        taken += 1;
    }
    Some(&items[taken..])
}

/// What a name of a space of tests under a prefix comes to under the other: the same
/// test there, a `to` that is no prefix of the space (why), or one of the other form
/// than `from`.
pub(super) enum Rename {
    /// The text of the name, built of the parts as written, and the parts it is to be
    /// read back as (the file's path, then each segment or title as written): a name
    /// that reads as other parts is none of the test.
    Built {
        text: String,
        parts: Vec<String>,
    },
    NoPrefix(String),
    OtherForm,
}

/// What an adapter of a space of tests (`rust-test`, `js-test`) says of a move of a
/// prefix, for the `names` of its space asked about: the one shape both answer in,
/// each by its own grammar. `parse_prefix` reads a prefix of the space (one that
/// cannot be read as `from` is no prefix of the space's names), `rename` says what one
/// name comes to under `to` (none when it is not under `from`, or is no name), and
/// `read_back` reads a name built back by the grammar of the space, into the parts it
/// reads as (one it refuses, or that reads as other parts than were joined, is
/// unbuildable). `forms` names the two forms of a prefix, for a `to` of the other one.
pub(super) fn renamed_tests<P, E: std::fmt::Display>(
    names: &BTreeSet<ArtifactName>,
    of: &PrefixMove,
    forms: &str,
    parse_prefix: impl Fn(&str) -> Result<P, E>,
    rename: impl Fn(&P, &str, &Result<P, E>) -> Option<Rename>,
    read_back: impl Fn(&str) -> Result<Vec<String>, E>,
) -> Result<(SpacePrefixes, SpaceRenamed), ValueError> {
    let from = match parse_prefix(of.from().as_str()) {
        Ok(from) => from,
        Err(why) => {
            let why = NonBlank::new(format!(
                "`{}` is no prefix of a test name: {why}",
                of.from()
            ))?;
            return Ok((SpacePrefixes::NotUnder(why), SpaceRenamed::new()));
        }
    };
    let to = parse_prefix(of.to().as_str());
    let mut answers = SpaceRenamed::new();
    for name in names {
        let answer = match rename(&from, name.as_str(), &to) {
            None => Renamed::Outside,
            Some(Rename::OtherForm) => unbuildable(format!(
                "the new prefix `{}` is of another form than `{}` ({forms})",
                of.to(),
                of.from()
            ))?,
            Some(Rename::NoPrefix(why)) => {
                unbuildable(format!("`{}` is no prefix of a test name: {why}", of.to()))?
            }
            Some(Rename::Built { text, parts }) => match read_back(&text) {
                Ok(read) if read == parts => built(&text)?,
                Ok(_) => unbuildable(
                    "the name built under the new prefix reads as other parts than the ones \
                     joined (the new prefix does not end where a part of a name ends)"
                        .to_owned(),
                )?,
                Err(why) => unbuildable(format!(
                    "the name built under the new prefix is no test name: {why}"
                ))?,
            },
        };
        answers.insert(name.clone(), answer);
    }
    Ok((SpacePrefixes::Answered, answers))
}
