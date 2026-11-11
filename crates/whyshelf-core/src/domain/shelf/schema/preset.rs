//! Presets and the default of links to artifacts: a shorthand of the declaration,
//! put out in full by [`expand`] before the declaration is checked.
//!
//! A link to artifacts says its space, its comparison and its meaning. A *preset*
//! is a named set of these, so that a link says `preset = "<name>"` and only what
//! differs. The presets come from two places:
//!
//! - *built-in*: each adapter of a space offers its own, as values through the port
//!   that offers the spaces ([`BuiltinPresets`]); the domain knows none of them;
//! - *of the project*: declared with the books ([`PresetDecl`]), each optionally
//!   `from` another preset, built-in or of the project. How a preset of the project
//!   and a built-in one of the same name meet is [`expand`]'s rule.
//!
//! A *default* ([`DefaultsDecl::artifact`]) is the preset of a link to artifacts that
//! names neither a preset nor a space. With none declared there is no default, and
//! such a link names no space, which [`Schema::validate`](super::Schema::validate)
//! refuses: no space is the default of the model.
//!
//! ```text
//!   ShelfDecl (books with WrittenLinkDecl, presets, defaults)
//!        |            BuiltinPresets (from the adapters, through the port)
//!        v                 |
//!     expand <-------------+
//!        |
//!        v
//!   Expanded (books with LinkEntryDecl, and the preset of each link) --> Schema::validate
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::nonempty::NonEmptyVec;

use super::super::artifact::{Comparison, Space};
use super::{
    BookSchemaDecl, Cardinality, CompareDecl, CoverDecl, LinkEntryDecl, LinkTargetDecl, NamePlace,
    NameProblem, PresetName, Propagation, SchemaProblem, offered,
};

/// The values a preset gives a link to artifacts, as written: each is given only
/// when it is written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresetValues {
    /// The space.
    pub space: Option<String>,
    /// `bytes`, `lines` or `presence`.
    pub compare: Option<String>,
    /// The prefixes of the lines `lines` leaves out.
    pub ignore: Option<Vec<String>>,
    /// What the link means; a preset cannot hold
    /// [`Replaces`](Propagation::Replaces) ([`SchemaProblem::PresetReplaces`]).
    pub means: Option<Propagation>,
}

impl PresetValues {
    /// These values, with those of `base` where these write none, `compare` and
    /// `ignore` taken as one value ([`expand`] states the rule).
    fn over(self, base: &Self) -> Self {
        let (compare, ignore) = match self.compare {
            Some(kind) => (Some(kind), self.ignore),
            None => (
                base.compare.clone(),
                self.ignore.or_else(|| base.ignore.clone()),
            ),
        };
        Self {
            space: self.space.or_else(|| base.space.clone()),
            compare,
            ignore,
            means: self.means.or(base.means),
        }
    }
}

/// The presets the adapters offer, by name: a value the port gives beside the
/// spaces, as the spaces are.
pub type BuiltinPresets = BTreeMap<PresetName, PresetValues>;

/// A preset a project declares, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetDecl {
    /// Its name.
    pub name: String,
    /// The preset whose values it takes where it writes none: a built-in one or
    /// another of the project.
    pub from: Option<String>,
    /// Its own values.
    pub values: PresetValues,
}

/// The defaults a project declares, as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultsDecl {
    /// The preset of a link to artifacts that names neither a preset nor a space.
    pub artifact: Option<String>,
}

/// A link as written: what [`expand`] reads. Each value the link writes itself wins
/// over the preset's; `means` may be left to the preset, and `count` is the link's
/// own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenLinkDecl {
    /// The name of the link.
    pub name: String,
    /// What it points at.
    pub target: LinkTargetDecl,
    /// How many links a document holds.
    pub cardinality: Cardinality,
    /// What the link means for freshness, when written.
    pub propagation: Option<Propagation>,
    /// The preset it names, for a link to artifacts.
    pub preset: Option<String>,
    /// For a link to artifacts, the space its names are in, when written.
    pub space: Option<String>,
    /// For a link to artifacts, `bytes`, `lines` or `presence`, when written.
    pub compare: Option<String>,
    /// For a link to artifacts, the prefixes of the lines `lines` leaves out, when
    /// written.
    pub ignore: Option<Vec<String>>,
    /// For a link into a book: how strongly every document of the target book is
    /// expected to be reached by it, when written.
    pub cover: Option<CoverDecl>,
}

impl From<LinkEntryDecl> for WrittenLinkDecl {
    /// The link written out in full, naming no preset: it expands to itself.
    fn from(link: LinkEntryDecl) -> Self {
        let (compare, ignore) = match link.compare {
            None => (None, None),
            Some(CompareDecl { kind, ignore }) => (Some(kind), Some(ignore)),
        };
        Self {
            name: link.name,
            target: link.target,
            cardinality: link.cardinality,
            propagation: Some(link.propagation),
            preset: None,
            space: link.space,
            compare,
            ignore,
            cover: link.cover,
        }
    }
}

/// A book as written, with its links as written.
pub type WrittenBookDecl = BookSchemaDecl<WrittenLinkDecl>;

/// The declaration of a project as an adapter read it: the books, the presets and
/// the defaults, nothing checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShelfDecl {
    /// The books.
    pub books: Vec<WrittenBookDecl>,
    /// The presets of the project, in the order written.
    pub presets: Vec<PresetDecl>,
    /// The defaults.
    pub defaults: DefaultsDecl,
}

impl ShelfDecl {
    /// A declaration of these books, written out in full: no preset, no default.
    pub fn of_books(books: Vec<BookSchemaDecl>) -> Self {
        Self {
            books: books
                .into_iter()
                .map(|book| BookSchemaDecl {
                    name: book.name,
                    dir: book.dir,
                    key: book.key,
                    description: book.description,
                    fields: book.fields,
                    links: book.links.into_iter().map(WrittenLinkDecl::from).collect(),
                })
                .collect(),
            presets: Vec::new(),
            defaults: DefaultsDecl::default(),
        }
    }
}

/// The preset a link to artifacts was expanded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetOrigin {
    /// The preset the link names.
    Named(PresetName),
    /// The default, for a link that names neither a preset nor a space.
    Default(PresetName),
}

impl PresetOrigin {
    /// The name of the preset.
    pub fn name(&self) -> &PresetName {
        match self {
            Self::Named(name) | Self::Default(name) => name,
        }
    }

    /// Whether it is the default.
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Default(_))
    }
}

/// Where a preset is named, for a problem to say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetPlace {
    /// `preset` of a link.
    Link {
        /// The book, as written.
        book: String,
        /// The link, as written.
        link: String,
    },
    /// `from` of a preset of the project.
    From {
        /// The preset, as written.
        preset: String,
    },
    /// The default of links to artifacts.
    Default,
}

impl fmt::Display for PresetPlace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Link { book, link } => write!(f, "book `{book}`: the link `{link}`"),
            Self::From { preset } => write!(f, "the `from` of the preset `{preset}`"),
            Self::Default => f.write_str("the default `artifact`"),
        }
    }
}

/// The preset of each link to artifacts that has one, by the book and the link as
/// written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Origins(BTreeMap<(String, String), PresetOrigin>);

impl Origins {
    pub(super) fn of(&self, book: &str, link: &str) -> Option<PresetOrigin> {
        self.0.get(&(book.to_owned(), link.to_owned())).cloned()
    }
}

/// A declaration with no preset left in it: what [`expand`] gives, and what
/// [`Schema::validate`](super::Schema::validate) checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    books: Vec<BookSchemaDecl>,
    origins: Origins,
}

impl Expanded {
    /// Declarations that name no preset.
    pub(super) fn plain(books: Vec<BookSchemaDecl>) -> Self {
        Self {
            books,
            origins: Origins::default(),
        }
    }

    /// The books, with every value of a preset put into their links.
    pub fn books(&self) -> &[BookSchemaDecl] {
        &self.books
    }

    /// The preset the link `link` of the book `book` (both as written) was expanded
    /// from, if any.
    pub fn origin(&self, book: &str, link: &str) -> Option<PresetOrigin> {
        self.origins.of(book, link)
    }

    pub(super) fn into_parts(self) -> (Vec<BookSchemaDecl>, Origins) {
        (self.books, self.origins)
    }
}

/// The declaration with every preset put out in full: the one place a preset is
/// read.
///
/// The rule, stated here once: each link to artifacts takes the values of the
/// preset it names, or, when it names neither a preset nor a space, of the default
/// (when one is declared). A value is taken from the first that writes it, in this
/// order:
///
/// 1. the link itself;
/// 2. the preset of the project of that name: its own values, then those of its
///    `from`, and so on along the chain;
/// 3. the built-in preset of that name.
///
/// A name, the one a link, a `from` or the default writes, is a preset of the
/// project when one is declared, and the built-in one of the name only when none
/// is: a preset of the project of the name of a built-in one, and with no `from`,
/// replaces the built-in one whole, and takes none of its values. The one
/// exception is a `from` that names the preset's own name: it names the built-in
/// one of that name, so `name = "file", from = "file"` refines the built-in `file`.
///
/// `space` and `means` are each one value. `compare` and `ignore` are one value
/// together: one that writes `compare` takes no `ignore` from what it comes from,
/// and has only the `ignore` it writes itself (none when it writes none); one that
/// writes `ignore` alone keeps the `compare` it comes from, with that `ignore`.
///
/// Only the expanded declaration is checked, answered and used: a pin, a cause and
/// a notice see the values, never a preset's name, so a preset that is edited
/// changes how the next approval reads and never how an old pin does (a comparison
/// that changes is told as any other). A link into a book takes no preset and no
/// default.
///
/// Every problem is returned: a preset name written twice or breaking the grammar
/// of names, a `from`, a link or a default that names a preset that is not there, a
/// cycle of `from`, a preset that means `replaces`, the values of a preset of the
/// project, checked on it whether a link uses it or not (a `space` that is none or
/// that `spaces` does not hold, and a `compare` with its `ignore` that is no
/// comparison), `preset` on a link into a book, and a link left with no `means`. A
/// preset with a problem gives no value: the links that use it, and the presets
/// that come from it, are left out with no problem of their own. The problems of a
/// link's values once expanded (a space not offered, a comparison that is none) are
/// [`Schema::validate`](super::Schema::validate)'s, on the link; since the values
/// of a preset were checked on it, they are of what the link itself writes.
// The problems are the answer, read once per run; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn expand(
    decl: ShelfDecl,
    builtins: &BuiltinPresets,
    spaces: &BTreeSet<Space>,
) -> Result<Expanded, NonEmptyVec<SchemaProblem>> {
    let (expanded, problems) = expand_all(decl, builtins, spaces);
    match NonEmptyVec::from_vec(problems) {
        Ok(problems) => Err(problems),
        Err(_) => Ok(expanded),
    }
}

/// What [`expand`] gives, with its problems beside what did expand: the links it
/// could not expand are left out, so the rest can be checked in the same pass.
pub(super) fn expand_all(
    decl: ShelfDecl,
    builtins: &BuiltinPresets,
    spaces: &BTreeSet<Space>,
) -> (Expanded, Vec<SchemaProblem>) {
    let mut problems = Vec::new();
    let mut presets = Presets::read(&decl.presets, builtins, spaces, &mut problems);
    let default = match decl.defaults.artifact.as_deref() {
        None => TheDefault::None,
        Some(name) => match presets.resolve(name, &mut problems) {
            Found::Values(values) => match presets.name(name) {
                Some(name) => TheDefault::Preset(name, values),
                None => TheDefault::Failed,
            },
            Found::Failed => TheDefault::Failed,
            Found::Unknown => {
                problems.push(SchemaProblem::UnknownPreset {
                    place: PresetPlace::Default,
                    name: name.to_owned(),
                });
                TheDefault::Failed
            }
        },
    };
    let mut origins = Origins::default();
    let mut books = Vec::new();
    for book in decl.books {
        let mut links = Vec::new();
        for link in book.links {
            let expanded = expand_link(&book.name, link, &mut presets, &default, &mut problems);
            if let Some((link, origin)) = expanded {
                if let Some(origin) = origin {
                    origins
                        .0
                        .insert((book.name.clone(), link.name.clone()), origin);
                }
                links.push(link);
            }
        }
        books.push(BookSchemaDecl {
            name: book.name,
            dir: book.dir,
            key: book.key,
            description: book.description,
            fields: book.fields,
            links,
        });
    }
    (Expanded { books, origins }, problems)
}

/// One link, expanded, with the preset it came from; none when it has a problem
/// (reported).
fn expand_link(
    book: &str,
    link: WrittenLinkDecl,
    presets: &mut Presets<'_>,
    default: &TheDefault,
    problems: &mut Vec<SchemaProblem>,
) -> Option<(LinkEntryDecl, Option<PresetOrigin>)> {
    let (values, origin) = match (&link.target, link.preset.as_deref()) {
        (LinkTargetDecl::Book(_), Some(_)) => {
            problems.push(SchemaProblem::PresetOnBookLink {
                book: book.to_owned(),
                link: link.name,
            });
            return None;
        }
        (LinkTargetDecl::Book(_), None) => (PresetValues::default(), None),
        (LinkTargetDecl::Artifact, Some(name)) => match presets.resolve(name, problems) {
            Found::Values(values) => (values, presets.name(name).map(PresetOrigin::Named)),
            // Its problem is reported where the preset is declared.
            Found::Failed => return None,
            Found::Unknown => {
                problems.push(SchemaProblem::UnknownPreset {
                    place: PresetPlace::Link {
                        book: book.to_owned(),
                        link: link.name,
                    },
                    name: name.to_owned(),
                });
                return None;
            }
        },
        (LinkTargetDecl::Artifact, None) if link.space.is_none() => match default {
            TheDefault::Preset(name, values) => {
                (values.clone(), Some(PresetOrigin::Default(name.clone())))
            }
            // A default that comes to nothing is reported once, at the default.
            TheDefault::Failed => return None,
            TheDefault::None => (PresetValues::default(), None),
        },
        (LinkTargetDecl::Artifact, None) => (PresetValues::default(), None),
    };
    let own = PresetValues {
        space: link.space,
        compare: link.compare,
        ignore: link.ignore,
        means: link.propagation,
    };
    let values = own.over(&values);
    let Some(propagation) = values.means else {
        problems.push(SchemaProblem::NoMeans {
            book: book.to_owned(),
            link: link.name,
        });
        return None;
    };
    let expanded = LinkEntryDecl {
        name: link.name,
        target: link.target,
        cardinality: link.cardinality,
        propagation,
        space: values.space,
        // `ignore` without `compare` is a `compare` of no kind, which
        // `Schema::validate` refuses, so it is not lost.
        compare: match (values.compare, values.ignore) {
            (None, None) => None,
            (kind, ignore) => Some(CompareDecl {
                kind: kind.unwrap_or_default(),
                ignore: ignore.unwrap_or_default(),
            }),
        },
        cover: link.cover,
    };
    Some((expanded, origin))
}

/// What a name of a preset comes to.
enum Found {
    /// Its values, with those of its chain put in.
    Values(PresetValues),
    /// A preset there, with a problem of its own or of its chain (reported).
    Failed,
    /// No preset of the name, of the project or built-in.
    Unknown,
}

/// The default of links to artifacts, resolved.
enum TheDefault {
    /// None is declared.
    None,
    /// This preset, with its values.
    Preset(PresetName, PresetValues),
    /// One is declared and comes to nothing (reported).
    Failed,
}

/// The presets of the project and the built-in ones, each resolved once.
struct Presets<'a> {
    /// The presets of the project with a sound name, the first of each name, with
    /// where it is in the declaration.
    project: BTreeMap<&'a str, (usize, PresetName, &'a PresetDecl)>,
    /// Names of the project that have a problem of their own (reported).
    broken: BTreeSet<&'a str>,
    builtins: &'a BuiltinPresets,
    /// The spaces offered, which a preset's `space` is checked against.
    spaces: &'a BTreeSet<Space>,
    /// The presets resolved so far: their values, or none when they failed.
    done: BTreeMap<String, Option<PresetValues>>,
}

impl<'a> Presets<'a> {
    /// The presets of the project, each checked for a name, a duplicate, a
    /// `replaces` and its values, and every one of them resolved, so that a problem
    /// of a preset no link names is reported too.
    fn read(
        decls: &'a [PresetDecl],
        builtins: &'a BuiltinPresets,
        spaces: &'a BTreeSet<Space>,
        problems: &mut Vec<SchemaProblem>,
    ) -> Self {
        let mut presets = Self {
            project: BTreeMap::new(),
            broken: BTreeSet::new(),
            builtins,
            spaces,
            done: BTreeMap::new(),
        };
        for (at, decl) in decls.iter().enumerate() {
            let name = match PresetName::new(&decl.name) {
                Ok(name) => name,
                Err(why) => {
                    problems.push(bad_name(&decl.name, why));
                    presets.broken.insert(&decl.name);
                    continue;
                }
            };
            if presets.project.contains_key(decl.name.as_str()) {
                problems.push(SchemaProblem::DuplicatePreset { name });
                continue;
            }
            if decl.values.means == Some(Propagation::Replaces) {
                problems.push(SchemaProblem::PresetReplaces {
                    preset: decl.name.clone(),
                });
                presets.broken.insert(&decl.name);
            }
            presets.project.insert(&decl.name, (at, name, decl));
        }
        for decl in decls {
            presets.resolve(&decl.name, problems);
        }
        presets
    }

    /// The checked name of a preset that is there.
    fn name(&self, name: &str) -> Option<PresetName> {
        match self.project.get(name) {
            Some((_, checked, _)) => Some(checked.clone()),
            None => self
                .builtins
                .get_key_value(name)
                .map(|(checked, _)| checked.clone()),
        }
    }

    /// What a name comes to.
    fn resolve(&mut self, name: &str, problems: &mut Vec<SchemaProblem>) -> Found {
        self.resolve_from(name, &mut Vec::new(), problems)
    }

    fn resolve_from(
        &mut self,
        name: &str,
        chain: &mut Vec<&'a str>,
        problems: &mut Vec<SchemaProblem>,
    ) -> Found {
        if let Some(done) = self.done.get(name) {
            return done.clone().map_or(Found::Failed, Found::Values);
        }
        if self.broken.contains(name) && !self.project.contains_key(name) {
            return Found::Failed;
        }
        let Some(&(_, _, decl)) = self.project.get(name) else {
            return self.builtin(name);
        };
        if let Some(at) = chain.iter().position(|seen| *seen == name) {
            let on_cycle = &chain[at..];
            // Told from the one of the cycle declared first, along `from`.
            let first = on_cycle
                .iter()
                .enumerate()
                .min_by_key(|(_, seen)| self.project.get(**seen).map(|(i, _, _)| *i))
                .map_or(0, |(i, _)| i);
            let cycle: Vec<PresetName> = on_cycle[first..]
                .iter()
                .chain(&on_cycle[..first])
                .filter_map(|seen| self.name(seen))
                .collect();
            if let Ok(presets) = NonEmptyVec::from_vec(cycle) {
                problems.push(SchemaProblem::PresetCycle { presets });
            }
            for seen in on_cycle {
                self.done.insert((*seen).to_owned(), None);
            }
            return Found::Failed;
        }
        chain.push(&decl.name);
        let base = match decl.from.as_deref() {
            None => Found::Values(PresetValues::default()),
            // Its own name: the built-in one it refines.
            Some(from) if from == decl.name => self.builtin(from),
            Some(from) => self.resolve_from(from, chain, problems),
        };
        let base = match (base, decl.from.as_deref()) {
            (Found::Unknown, Some(from)) => {
                problems.push(SchemaProblem::UnknownPreset {
                    place: PresetPlace::From {
                        preset: decl.name.clone(),
                    },
                    name: from.to_owned(),
                });
                Found::Failed
            }
            (found, _) => found,
        };
        chain.pop();
        // A preset on a cycle was settled as failed by the one that found the cycle.
        if let Some(done) = self.done.get(name) {
            return done.clone().map_or(Found::Failed, Found::Values);
        }
        let values = match base {
            Found::Values(base) if !self.broken.contains(name) => {
                let values = decl.values.clone().over(&base);
                self.sound(decl, &values, problems).then_some(values)
            }
            _ => None,
        };
        self.done.insert(name.to_owned(), values.clone());
        values.map_or(Found::Failed, Found::Values)
    }

    /// The built-in preset of a name.
    fn builtin(&self, name: &str) -> Found {
        match self.builtins.get(name) {
            Some(values) => Found::Values(values.clone()),
            None => Found::Unknown,
        }
    }

    /// Whether the values a preset of the project writes are sound, as it writes
    /// them over those it comes from (`values`): its `space` a space that is
    /// offered, and its `compare` and `ignore`, when it writes either, a comparison.
    /// Each problem is reported, naming the preset.
    fn sound(
        &self,
        decl: &PresetDecl,
        values: &PresetValues,
        problems: &mut Vec<SchemaProblem>,
    ) -> bool {
        let before = problems.len();
        if let Some(written) = decl.values.space.as_deref() {
            match Space::new(written) {
                Ok(space) if self.spaces.contains(&space) => {}
                Ok(space) => problems.push(SchemaProblem::PresetUnknownSpace {
                    preset: decl.name.clone(),
                    space,
                    offered: offered(self.spaces),
                }),
                Err(why) => problems.push(SchemaProblem::PresetBadSpace {
                    preset: decl.name.clone(),
                    why,
                }),
            }
        }
        if decl.values.compare.is_some() || decl.values.ignore.is_some() {
            let kind = values.compare.as_deref().unwrap_or_default();
            let ignore = values.ignore.as_deref().unwrap_or_default();
            if let Err(why) = Comparison::parse(kind, ignore) {
                problems.push(SchemaProblem::PresetBadComparison {
                    preset: decl.name.clone(),
                    why,
                });
            }
        }
        problems.len() == before
    }
}

fn bad_name(text: &str, why: NameProblem) -> SchemaProblem {
    SchemaProblem::BadName {
        place: NamePlace::Preset,
        text: text.to_owned(),
        why,
    }
}
