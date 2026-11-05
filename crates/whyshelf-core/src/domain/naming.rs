//! Naming: how a person or a document points at a document or an item, and how
//! the pointer is looked up in the [`Corpus`].
//!
//! There is one grammar. Every command argument and every reference inside a
//! document is read by [`Ref::parse`], and the [`Place`] it is read for decides which
//! forms it may take. The old model's [`reference::Ref`](super::reference::Ref) is
//! another type and stays beside this one until the application moves over.
//!
//! # The forms
//!
//! | form | written | points at |
//! |---|---|---|
//! | [`Ref::Id`] | a UUID v7, lowercase and hyphenated | the document with that id |
//! | [`Ref::Short`] | 8 to 32 lowercase hexadecimal characters | the document whose id **ends** with them ([`ShortId`]) |
//! | [`Ref::Alias`] | `spec/cache-timeout` | the document that has that alias |
//! | [`Ref::Path`] | `docs/cache.md` | the document whose file is at that path, in either root |
//! | [`Ref::Item`] | `<design>#12` | the item numbered 12 in that Design, now |
//! | [`Ref::ItemId`] | `<design>#id:9f3c21a7` | the item of that Design whose id ends with the short id |
//! | [`Ref::Private`] | `private:<id>` | a document by its opaque id, the way a public text may name a private one |
//! | [`Ref::Ext`] | `ext:<project>/<id>` | a document of another project |
//! | [`Ref::Url`] | `https://example.org/x` | something outside the model |
//!
//! The two item forms are told apart by their mark: what follows the `#` is either
//! decimal digits only or `id:` and a short id, so `#12345678` is the item numbered
//! twelve million and `#id:12345678` is the item whose id ends in those digits.
//!
//! A bare text is read in this order: a hyphenated UUID is an id (and an error if it
//! is not a lowercase v7); then 8 to 32 hexadecimal characters, in either case, are a
//! short id (and [`RefError::Short`] if one of them is uppercase, so `9F3C21A7` is
//! refused and never read as a path); then a text that is a valid alias is an alias,
//! unless it ends in `.md`, when it is a path; then a text that is a valid path is a
//! path. No alias is 8 to 32 hexadecimal characters ([`Alias`] refuses it), so a
//! bare hexadecimal text is never ambiguous between an alias and a short id; `cafe-babe`
//! and `v20240101` are aliases. A short id shorter than 8 characters is refused where
//! a `private:` or `#id:` names it, and elsewhere is read as the alias it may be.
//!
//! # Where each form is accepted
//!
//! | form | [`Place::Argument`] | [`Place::Key`] (`grounds`, `spec`) | [`Place::Body`] (inside a text) |
//! |---|---|---|---|
//! | Id, Alias | yes | yes | yes |
//! | `private:<id>` | yes, and with a short id | yes, with a full id only | yes, with a full id only |
//! | Short | yes | **no**: [`RefError::ShortIdInDocument`] | **no**: the same |
//! | Path | yes | no | no |
//! | `<design>#<n>` | yes | no | yes, with a [`NamingLint::FragileItemRef`] |
//! | `<design>#id:<short>` | yes | no | yes (the design is named by id or alias) |
//! | Ext, Url | yes | no (a document that stands on another project's document could not be approved anywhere) | yes |
//!
//! Why a document may not hold a short id or a path. Where the private root is not
//! readable, a short id cannot be turned into a full id, so no holder of an
//! unresolved reference and no keyed pin could be made for it. And when an id that
//! ends alike is made later, a reference that was written earlier would turn
//! ambiguous because of somebody else's new document. A path breaks when a file is
//! moved. A command argument is resolved on the spot, so neither applies.
//!
//! A `<design>#id:<short>` is the one place a short id lives in a file, and there it
//! is unambiguous because the short ids of the items of one Design are unique: a
//! Design where two items end alike in their last 8 characters is Invalid
//! ([`ItemIssue::DuplicateItemShortId`](super::design::ItemIssue)).
//!
//! # Looking up: [`Resolver`]
//!
//! A reference is looked up in the corpus (both roots, by id, so a document that
//! was published is found at its new place) and gives a [`Resolution`]:
//!
//! | resolution | meaning |
//! |---|---|
//! | `Resolved` | exactly one document or item |
//! | `Ambiguous` | several: the candidates are listed, none is chosen |
//! | `Dangling` | nothing has that name here and the corpus can say so; a [`Rename`] says so when an earlier name is known |
//! | `Unresolved` | it cannot be told here: a `private:` that names nothing here, `ext:`, a URL, an item that has no `id:` yet |
//!
//! A `private:<id>` that names nothing in the corpus is always `Unresolved`, never
//! `Dangling`, whatever this person's [`PrivateRoot`](super::boundary::PrivateRoot) is: the private documents of
//! someone else are never visible here, so "there is no such document" cannot be
//! told from "it is someone else's". (A clone that cannot see the private documents
//! must stay usable too.) The resolver therefore does not look at the private root.
//!
//! The renames come from two histories the application builds from the stored full
//! texts of past versions and passes in as values: [`AliasHistory`] (the aliases a
//! document has had) and [`ItemNumberHistory`] (the numbers an item has had). A
//! purged version leaves no entry. A hint is given only when exactly one document
//! or item fits.
//!
//! A number that was given up and given again to another item cannot be told from
//! the number it was: nothing in `<design>#3` says which time it means, and time does
//! not order anything here. That is why a number in a document is a warning
//! ([`NamingLint::FragileItemRef`]) and why a person who types one is answered with
//! today's item.
//!
//! # Lints of a reference in a document
//!
//! [`document_lints`] finds what a reference written into a document should not be:
//! a `<design>#<number>` (fragile), and a `private:<id>` whose document is public
//! now (the mark is stale; the text should name the document without it).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use super::corpus::{Corpus, Entity};
use super::design::Design;
use super::error::RefError;
use super::id::{Alias, DocId, ItemId, ItemNumber, ShortId};
use super::location::{RelPath, Uri};
use super::nonempty::NonEmptyVec;
use super::reference::ProjectName;

/// Where a reference is written: the place decides which forms may stand there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Place {
    /// A command argument: a person typing, answered on the spot.
    Argument,
    /// The value of a key that binds an upstream in a document (`grounds`, `spec`).
    Key,
    /// A reference inside the text of a document.
    Body,
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Argument => "a command argument",
            Self::Key => "a key that binds an upstream",
            Self::Body => "the text of a document",
        })
    }
}

/// Which form a [`Ref`] is, without what it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefForm {
    /// [`Ref::Id`].
    Id,
    /// [`Ref::Short`].
    Short,
    /// [`Ref::Alias`].
    Alias,
    /// [`Ref::Path`].
    Path,
    /// [`Ref::Item`].
    Item,
    /// [`Ref::ItemId`].
    ItemId,
    /// [`Ref::Private`].
    Private,
    /// [`Ref::Ext`].
    Ext,
    /// [`Ref::Url`].
    Url,
}

impl fmt::Display for RefForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Id => "id",
            Self::Short => "short id",
            Self::Alias => "alias",
            Self::Path => "path",
            Self::Item => "item number",
            Self::ItemId => "item id",
            Self::Private => "`private:` reference",
            Self::Ext => "`ext:` reference",
            Self::Url => "URL",
        })
    }
}

/// A reference as written. See the module documentation for the grammar and for
/// where each form is accepted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ref {
    /// A document by its full id.
    Id(DocId),
    /// A document by the end of its id.
    Short(ShortId),
    /// A document by its alias.
    Alias(Alias),
    /// A document by the path of its file.
    Path(RelPath),
    /// An item by its number in a Design: `<design>#<n>`.
    Item(Box<Ref>, ItemNumber),
    /// An item by the end of its id, in a Design: `<design>#id:<short>`.
    ItemId(Box<Ref>, ShortId),
    /// A document by its opaque id: `private:<id>`. The inner reference is an
    /// [`Ref::Id`], or a [`Ref::Short`] where a person types it.
    Private(Box<Ref>),
    /// A document of another project: `ext:<project>/<id>`.
    Ext {
        /// The other project.
        project: ProjectName,
        /// The document in it.
        id: DocId,
    },
    /// Something outside the model.
    Url(Uri),
}

impl Ref {
    /// Read a reference for a place: the one parser of command arguments and of
    /// references in documents.
    ///
    /// The text is read by the grammar first, and then the place decides whether the
    /// form may stand there ([`RefError::ShortIdInDocument`] for a short id in a
    /// document, [`RefError::NotAllowedHere`] for another form that does not belong).
    pub fn parse(text: &str, place: Place) -> Result<Self, RefError> {
        let reference = Self::read(text)?;
        reference.accepted_at(place)?;
        Ok(reference)
    }

    /// Which form this is.
    pub fn form(&self) -> RefForm {
        match self {
            Self::Id(_) => RefForm::Id,
            Self::Short(_) => RefForm::Short,
            Self::Alias(_) => RefForm::Alias,
            Self::Path(_) => RefForm::Path,
            Self::Item(..) => RefForm::Item,
            Self::ItemId(..) => RefForm::ItemId,
            Self::Private(_) => RefForm::Private,
            Self::Ext { .. } => RefForm::Ext,
            Self::Url(_) => RefForm::Url,
        }
    }

    /// Whether the form may stand in `place`; the table of the module documentation.
    pub fn accepted_at(&self, place: Place) -> Result<(), RefError> {
        if place == Place::Argument {
            return Ok(());
        }
        match self {
            Self::Id(_) | Self::Alias(_) => Ok(()),
            Self::Short(short) => Err(RefError::ShortIdInDocument {
                short: short.clone(),
                place,
            }),
            Self::Path(_) => Err(self.not_allowed(place)),
            Self::Item(base, _) | Self::ItemId(base, _) => match place {
                Place::Key | Place::Argument => Err(self.not_allowed(place)),
                Place::Body => base.accepted_at(place),
            },
            Self::Private(inner) => inner.accepted_at(place),
            Self::Ext { .. } | Self::Url(_) => match place {
                Place::Key => Err(self.not_allowed(place)),
                Place::Body | Place::Argument => Ok(()),
            },
        }
    }

    fn not_allowed(&self, place: Place) -> RefError {
        RefError::NotAllowedHere {
            form: self.form(),
            place,
        }
    }

    fn read(text: &str) -> Result<Self, RefError> {
        if text.is_empty() {
            return Err(RefError::Empty);
        }
        if text.contains("://") {
            return text.parse::<Uri>().map(Self::Url).map_err(RefError::Url);
        }
        if let Some(rest) = text.strip_prefix("ext:") {
            let (project, id) = rest.split_once('/').ok_or(RefError::ExtShape)?;
            return Ok(Self::Ext {
                project: project.parse().map_err(RefError::Project)?,
                id: id.parse().map_err(RefError::ExtId)?,
            });
        }
        let Some((base, selector)) = text.rsplit_once('#') else {
            return Self::read_base(text);
        };
        let base = Box::new(Self::read_base(base)?);
        match selector.strip_prefix("id:") {
            Some(short) => Ok(Self::ItemId(base, short.parse().map_err(RefError::Short)?)),
            None => {
                let digits = !selector.is_empty() && selector.bytes().all(|b| b.is_ascii_digit());
                if !digits {
                    return Err(RefError::ItemSelector {
                        text: selector.to_owned(),
                    });
                }
                let number = selector.parse().map_err(RefError::ItemNumber)?;
                Ok(Self::Item(base, number))
            }
        }
    }

    /// A reference that can be the base of an item: one that names a document.
    fn read_base(text: &str) -> Result<Self, RefError> {
        if let Some(rest) = text.strip_prefix("private:") {
            let inner = if rest.contains('-') {
                Self::Id(rest.parse().map_err(RefError::PrivateTarget)?)
            } else {
                Self::Short(rest.parse().map_err(RefError::PrivateTarget)?)
            };
            return Ok(Self::Private(Box::new(inner)));
        }
        if text.contains('-') && uuid::Uuid::parse_str(text).is_ok() {
            return text.parse().map(Self::Id).map_err(RefError::Id);
        }
        // Either case: an uppercase one is refused by `ShortId` and not read as a path.
        if ShortId::looks_like(text) {
            return text.parse().map(Self::Short).map_err(RefError::Short);
        }
        match (text.parse::<Alias>(), text.parse::<RelPath>()) {
            (Ok(_), Ok(path)) if text.ends_with(".md") => Ok(Self::Path(path)),
            (Ok(alias), _) => Ok(Self::Alias(alias)),
            (Err(_), Ok(path)) => Ok(Self::Path(path)),
            (Err(cause), Err(_)) => Err(RefError::Name(cause)),
        }
    }
}

impl FromStr for Ref {
    type Err = RefError;

    /// A reference typed as a command argument: every form is accepted.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text, Place::Argument)
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(id) => fmt::Display::fmt(id, f),
            Self::Short(short) => fmt::Display::fmt(short, f),
            Self::Alias(alias) => fmt::Display::fmt(alias, f),
            Self::Path(path) => fmt::Display::fmt(path, f),
            Self::Item(base, number) => write!(f, "{base}{number}"),
            Self::ItemId(base, short) => write!(f, "{base}#id:{short}"),
            Self::Private(inner) => write!(f, "private:{inner}"),
            Self::Ext { project, id } => write!(f, "ext:{project}/{id}"),
            Self::Url(uri) => fmt::Display::fmt(uri, f),
        }
    }
}

/// A name an earlier version of a document or an item had, and what it is called
/// now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rename {
    /// A document's alias changed.
    Alias {
        /// The alias a reference still uses.
        was: Alias,
        /// The alias the document has now.
        now: Alias,
        /// The document.
        doc: DocId,
    },
    /// An item was renumbered.
    ItemNumber {
        /// The Design the item is in.
        design: DocId,
        /// The number a reference still uses.
        was: ItemNumber,
        /// The number the item has now.
        now: ItemNumber,
        /// The item.
        item: ItemId,
    },
}

/// What a lookup of a [`Ref`] gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution<T> {
    /// Exactly one.
    Resolved(T),
    /// Several fit, and none is chosen.
    Ambiguous(NonEmptyVec<T>),
    /// Nothing here has that name.
    Dangling {
        /// What the name used to belong to, when an earlier version says.
        hint: Option<Rename>,
    },
    /// The name cannot be looked up here.
    Unresolved,
}

/// What a reference names: a document or an item of a Design.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Named {
    /// A document.
    Doc(DocId),
    /// An item.
    Item(NamedItem),
}

/// An item found by a lookup: its identity, its Design and its number now.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamedItem {
    design: DocId,
    id: ItemId,
    number: ItemNumber,
}

impl NamedItem {
    /// The Design the item is in.
    pub fn design(&self) -> DocId {
        self.design
    }

    /// The identity of the item.
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// The number the item has now.
    pub fn number(&self) -> ItemNumber {
        self.number
    }
}

/// The aliases documents have had, built by the application from the stored full
/// texts of past versions.
///
/// A version whose text was purged leaves nothing here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AliasHistory {
    past: BTreeMap<DocId, BTreeSet<Alias>>,
}

impl AliasHistory {
    /// A history with no entry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Note that `doc` has had `alias`.
    pub fn record(&mut self, doc: DocId, alias: Alias) {
        self.past.entry(doc).or_default().insert(alias);
    }

    /// The documents that have had `alias`, in order.
    pub fn owners(&self, alias: &Alias) -> Vec<DocId> {
        self.past
            .iter()
            .filter(|(_, aliases)| aliases.contains(alias))
            .map(|(doc, _)| *doc)
            .collect()
    }
}

/// The numbers items have had in their Designs, built by the application from the
/// stored full texts of past versions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemNumberHistory {
    past: BTreeMap<(DocId, ItemId), BTreeSet<ItemNumber>>,
}

impl ItemNumberHistory {
    /// A history with no entry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Note that `item` has had `number` in `design`.
    pub fn record(&mut self, design: DocId, item: ItemId, number: ItemNumber) {
        self.past.entry((design, item)).or_default().insert(number);
    }

    /// Whether `item` has had `number` in `design`.
    pub fn had(&self, design: DocId, item: ItemId, number: ItemNumber) -> bool {
        self.past
            .get(&(design, item))
            .is_some_and(|numbers| numbers.contains(&number))
    }
}

/// Looks references up in a corpus. See the module documentation for the rules.
#[derive(Debug, Clone, Copy)]
pub struct Resolver<'a> {
    corpus: &'a Corpus,
    aliases: &'a AliasHistory,
    numbers: &'a ItemNumberHistory,
}

impl<'a> Resolver<'a> {
    /// A resolver over `corpus`. It does not depend on the person's
    /// [`PrivateRoot`](super::boundary::PrivateRoot): see the module documentation.
    pub fn new(
        corpus: &'a Corpus,
        aliases: &'a AliasHistory,
        numbers: &'a ItemNumberHistory,
    ) -> Self {
        Self {
            corpus,
            aliases,
            numbers,
        }
    }

    /// Look a reference up.
    pub fn resolve(&self, reference: &Ref) -> Resolution<Named> {
        match reference {
            Ref::Id(id) => self.by_id(*id),
            Ref::Short(short) => settle(self.by_end(short), || Resolution::Dangling { hint: None }),
            Ref::Alias(alias) => settle(self.by_alias_docs(alias), || Resolution::Dangling {
                hint: self.alias_hint(alias),
            }),
            Ref::Path(path) => settle(
                self.corpus
                    .documents()
                    .iter()
                    .filter(|file| file.location().path() == path)
                    .map(|file| file.id())
                    .collect(),
                || Resolution::Dangling { hint: None },
            ),
            Ref::Item(base, number) => {
                self.in_design(base, |design, found| match found.items().get(number) {
                    Some(item) => self.named_item(design, *number, item.id()),
                    None => Resolution::Dangling {
                        hint: self.number_hint(design, found, *number),
                    },
                })
            }
            Ref::ItemId(base, short) => self.in_design(base, |design, found| {
                let fits: Vec<Named> = found
                    .items()
                    .iter()
                    .filter_map(|(number, item)| {
                        let id = item.id().filter(|id| short.matches(id.entity()))?;
                        Some(Named::Item(NamedItem {
                            design,
                            id,
                            number: *number,
                        }))
                    })
                    .collect();
                one_of(fits, || Resolution::Dangling { hint: None })
            }),
            Ref::Private(inner) => {
                let found = match inner.as_ref() {
                    Ref::Id(id) => self.holders(*id),
                    Ref::Short(short) => self.by_end(short),
                    _ => BTreeSet::new(),
                };
                // Never `Dangling`: someone else's private document is not visible
                // here, so "not found" cannot be told from "not mine".
                settle(found, || Resolution::Unresolved)
            }
            Ref::Ext { .. } | Ref::Url(_) => Resolution::Unresolved,
        }
    }

    fn by_id(&self, id: DocId) -> Resolution<Named> {
        settle(self.holders(id), || Resolution::Dangling { hint: None })
    }

    /// `id` itself when a file has it (several files of one id are one document).
    fn holders(&self, id: DocId) -> BTreeSet<DocId> {
        self.corpus
            .by_id(&id)
            .first()
            .map(|file| file.id())
            .into_iter()
            .collect()
    }

    fn by_end(&self, short: &ShortId) -> BTreeSet<DocId> {
        self.corpus
            .documents()
            .iter()
            .map(|file| file.id())
            .filter(|id| short.matches(id.entity()))
            .collect()
    }

    fn by_alias_docs(&self, alias: &Alias) -> BTreeSet<DocId> {
        self.corpus
            .by_alias(alias)
            .into_iter()
            .map(|file| file.id())
            .collect()
    }

    /// The alias a document has had, if exactly one document that is here has had
    /// `alias` and has another now.
    fn alias_hint(&self, alias: &Alias) -> Option<Rename> {
        let mut renames = self.aliases.owners(alias).into_iter().filter_map(|doc| {
            let now = self
                .corpus
                .by_id(&doc)
                .first()?
                .document()
                .header()
                .alias()?;
            (now != alias).then(|| Rename::Alias {
                was: alias.clone(),
                now: now.clone(),
                doc,
            })
        });
        let hint = renames.next()?;
        renames.next().is_none().then_some(hint)
    }

    /// The number an item has now, if exactly one item of the Design has had `number`.
    fn number_hint(&self, design: DocId, found: &Design, number: ItemNumber) -> Option<Rename> {
        let mut renames = found.items().iter().filter_map(|(now, item)| {
            let item = item.id()?;
            (*now != number && self.numbers.had(design, item, number)).then_some(
                Rename::ItemNumber {
                    design,
                    was: number,
                    now: *now,
                    item,
                },
            )
        });
        let hint = renames.next()?;
        renames.next().is_none().then_some(hint)
    }

    /// An item found by number. One with no `id:` is not an item of the model yet.
    fn named_item(
        &self,
        design: DocId,
        number: ItemNumber,
        id: Option<ItemId>,
    ) -> Resolution<Named> {
        match id {
            Some(id) => Resolution::Resolved(Named::Item(NamedItem { design, id, number })),
            None => Resolution::Unresolved,
        }
    }

    /// Resolve `base` to a Design and ask `select` of it.
    fn in_design(
        &self,
        base: &Ref,
        select: impl FnOnce(DocId, &Design) -> Resolution<Named>,
    ) -> Resolution<Named> {
        match self.resolve(base) {
            Resolution::Resolved(Named::Doc(doc)) => {
                let design = self.corpus.by_id(&doc).first().and_then(|file| {
                    match file.document().entity() {
                        Entity::Design(design) => Some(design),
                        Entity::Finding(_) | Entity::Spec(_) => None,
                    }
                });
                match design {
                    Some(design) => select(doc, design),
                    None => Resolution::Dangling { hint: None },
                }
            }
            Resolution::Resolved(Named::Item(_)) => Resolution::Dangling { hint: None },
            Resolution::Ambiguous(candidates) => Resolution::Ambiguous(candidates),
            Resolution::Dangling { hint } => Resolution::Dangling { hint },
            Resolution::Unresolved => Resolution::Unresolved,
        }
    }
}

/// One document, several, or none (answered by `none`).
fn settle(found: BTreeSet<DocId>, none: impl FnOnce() -> Resolution<Named>) -> Resolution<Named> {
    one_of(found.into_iter().map(Named::Doc).collect(), none)
}

fn one_of(found: Vec<Named>, none: impl FnOnce() -> Resolution<Named>) -> Resolution<Named> {
    let mut found = found.into_iter();
    match (found.next(), found.next()) {
        (None, _) => none(),
        (Some(only), None) => Resolution::Resolved(only),
        (Some(first), Some(second)) => {
            let mut rest = vec![second];
            rest.extend(found);
            Resolution::Ambiguous(NonEmptyVec::new(first, rest))
        }
    }
}

/// What a reference written into a document should not be.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NamingLint {
    /// A reference to an item by its number.
    ///
    /// A number can be given up and given again to another item, and nothing in the
    /// reference says which time it means. Fix: name the item as
    /// `<design>#id:<short>`.
    #[error("`{reference}` names an item by a number, which can come to mean another item")]
    FragileItemRef {
        /// The reference as written.
        reference: Ref,
    },
    /// A `private:<id>` whose document is public now.
    ///
    /// Fix: name the document without the mark.
    #[error("`{reference}` marks {doc} private, and it is public now")]
    MarkStale {
        /// The reference as written.
        reference: Ref,
        /// The document it names.
        doc: DocId,
    },
}

/// The warnings for one reference written into a document.
///
/// `corpus` says whether a document named by `private:` is still private
/// ([`Corpus::is_private`]); a document that is not in the corpus is not judged.
pub fn document_lints(reference: &Ref, corpus: &Corpus) -> Vec<NamingLint> {
    let mut lints = Vec::new();
    collect_lints(reference, reference, corpus, &mut lints);
    lints
}

fn collect_lints(whole: &Ref, part: &Ref, corpus: &Corpus, out: &mut Vec<NamingLint>) {
    match part {
        Ref::Item(base, _) => {
            out.push(NamingLint::FragileItemRef {
                reference: whole.clone(),
            });
            collect_lints(whole, base, corpus, out);
        }
        Ref::ItemId(base, _) => collect_lints(whole, base, corpus, out),
        Ref::Private(inner) => {
            if let Ref::Id(doc) = inner.as_ref()
                && !corpus.by_id(doc).is_empty()
                && !corpus.is_private(doc)
            {
                out.push(NamingLint::MarkStale {
                    reference: whole.clone(),
                    doc: *doc,
                });
            }
        }
        Ref::Id(_)
        | Ref::Short(_)
        | Ref::Alias(_)
        | Ref::Path(_)
        | Ref::Ext { .. }
        | Ref::Url(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::corpus::{
        Document, FileFacts, GitPlace, Header, Location, Reading, Visibility,
    };
    use crate::domain::design::test_support::{design, identified, item, num};
    use crate::domain::error::ValueError;
    use crate::domain::event::DocKind;
    use crate::domain::finding::test_support::finding;
    use crate::domain::hash::BytesHash;
    use crate::domain::id::test_support::{docid, iid};
    use crate::domain::knowable::Knowable;
    use crate::domain::spec::test_support::spec;

    fn parse(text: &str) -> Ref {
        Ref::parse(text, Place::Argument).unwrap()
    }

    fn id_text(n: u128) -> String {
        docid(n).to_string()
    }

    // --- the grammar --------------------------------------------------------

    #[test]
    fn every_form_is_read_and_written_back() {
        let id = id_text(5);
        let cases = [
            (id.clone(), RefForm::Id),
            ("9f3c21a7".to_owned(), RefForm::Short),
            ("spec/cache-timeout".to_owned(), RefForm::Alias),
            ("docs/cache.md".to_owned(), RefForm::Path),
            (format!("{id}#12"), RefForm::Item),
            ("spec/cache-timeout#12".to_owned(), RefForm::Item),
            (format!("{id}#id:9f3c21a7"), RefForm::ItemId),
            (format!("private:{id}"), RefForm::Private),
            ("private:9f3c21a7".to_owned(), RefForm::Private),
            (format!("ext:other-project/{id}"), RefForm::Ext),
            ("https://example.org/a#b".to_owned(), RefForm::Url),
        ];
        for (text, form) in cases {
            let reference = parse(&text);
            assert_eq!(reference.form(), form, "{text}");
            assert_eq!(reference.to_string(), text);
            assert_eq!(text.parse::<Ref>().unwrap(), reference);
        }
    }

    #[test]
    fn a_number_and_an_item_id_are_told_apart_by_their_mark() {
        let base = id_text(5);
        assert_eq!(
            parse(&format!("{base}#12")),
            Ref::Item(Box::new(Ref::Id(docid(5))), num(12))
        );
        assert_eq!(
            parse(&format!("{base}#id:9f3c21a7")),
            Ref::ItemId(
                Box::new(Ref::Id(docid(5))),
                "9f3c21a7".parse::<ShortId>().unwrap()
            )
        );
        // Digits only after `#id:` are a short id, and digits only after `#` a number.
        let digits = parse(&format!("{base}#id:12345678"));
        assert!(matches!(digits, Ref::ItemId(_, ref s) if s.as_str() == "12345678"));
        let number = parse(&format!("{base}#12345678"));
        assert!(matches!(number, Ref::Item(_, n) if n.get() == 12_345_678));
    }

    #[test]
    fn a_short_id_that_is_too_short_is_refused_where_it_is_named() {
        let base = id_text(5);
        let short = |cause: ValueError| RefError::Short(cause);
        let too_short = |len| ValueError::ShortIdTooShort { len, min: 8 };
        assert_eq!(
            Ref::parse(&format!("{base}#id:9f3c21"), Place::Argument),
            Err(short(too_short(6)))
        );
        assert_eq!(
            Ref::parse(&format!("{base}#id:"), Place::Argument),
            Err(short(too_short(0)))
        );
        assert_eq!(
            Ref::parse("private:9f3c21", Place::Argument),
            Err(RefError::PrivateTarget(too_short(6)))
        );
        // Bare, a text that short is simply an alias.
        assert!(matches!(parse("9f3c21"), Ref::Alias(_)));
    }

    #[test]
    fn a_bad_item_selector_or_number_is_refused() {
        let base = id_text(5);
        assert_eq!(
            Ref::parse(&format!("{base}#abc"), Place::Argument),
            Err(RefError::ItemSelector { text: "abc".into() })
        );
        assert_eq!(
            Ref::parse(&format!("{base}#"), Place::Argument),
            Err(RefError::ItemSelector {
                text: String::new()
            })
        );
        assert_eq!(
            Ref::parse(&format!("{base}#0"), Place::Argument),
            Err(RefError::ItemNumber(ValueError::ItemNumberZero))
        );
        assert!(matches!(
            Ref::parse(&format!("{base}#99999999999"), Place::Argument),
            Err(RefError::ItemNumber(ValueError::ItemNumberNotNumber(_)))
        ));
    }

    #[test]
    fn text_that_is_no_reference_is_refused_with_its_reason() {
        assert_eq!(Ref::parse("", Place::Argument), Err(RefError::Empty));
        let upper = id_text(0xabc).to_uppercase();
        assert_eq!(
            Ref::parse(&upper, Place::Argument),
            Err(RefError::Id(ValueError::IdNotCanonical))
        );
        assert_eq!(
            Ref::parse("12345678-1234-4234-8234-123456789012", Place::Argument),
            Err(RefError::Id(ValueError::IdNotV7 { version: 4 }))
        );
        // Neither an alias nor a path (a text with capitals is a path, as `README.md` is).
        assert!(matches!(
            Ref::parse("/abs/path", Place::Argument),
            Err(RefError::Name(_))
        ));
        assert!(matches!(parse("Docs/README.txt"), Ref::Path(_)));
        assert_eq!(
            Ref::parse("ext:no-slash", Place::Argument),
            Err(RefError::ExtShape)
        );
        assert!(matches!(
            Ref::parse("ext:Bad Project/x", Place::Argument),
            Err(RefError::Project(_))
        ));
    }

    // --- where a form is accepted -------------------------------------------

    #[test]
    fn a_short_id_is_not_taken_in_a_document() {
        let short: ShortId = "9f3c21a7".parse().unwrap();
        for place in [Place::Key, Place::Body] {
            assert_eq!(
                Ref::parse("9f3c21a7", place),
                Err(RefError::ShortIdInDocument {
                    short: short.clone(),
                    place
                })
            );
            assert_eq!(
                Ref::parse("private:9f3c21a7", place),
                Err(RefError::ShortIdInDocument {
                    short: short.clone(),
                    place
                })
            );
        }
        // A design named by a short id is a short id in a document too.
        assert!(matches!(
            Ref::parse("9f3c21a7#id:9f3c21a7", Place::Body),
            Err(RefError::ShortIdInDocument { .. })
        ));
        assert!(Ref::parse("9f3c21a7", Place::Argument).is_ok());
    }

    #[test]
    fn a_key_takes_ids_aliases_and_private_ids_and_nothing_more() {
        let id = id_text(5);
        for ok in [id.clone(), "spec/a".to_owned(), format!("private:{id}")] {
            assert!(Ref::parse(&ok, Place::Key).is_ok(), "{ok}");
        }
        for (text, form) in [
            ("docs/a.md".to_owned(), RefForm::Path),
            (format!("ext:p/{id}"), RefForm::Ext),
            ("https://example.org/a".to_owned(), RefForm::Url),
            (format!("{id}#3"), RefForm::Item),
            (format!("{id}#id:9f3c21a7"), RefForm::ItemId),
        ] {
            assert_eq!(
                Ref::parse(&text, Place::Key),
                Err(RefError::NotAllowedHere {
                    form,
                    place: Place::Key
                }),
                "{text}"
            );
        }
    }

    #[test]
    fn a_text_takes_external_references_and_item_references_but_no_path() {
        let id = id_text(5);
        for ok in [
            format!("ext:p/{id}"),
            "https://example.org/a".to_owned(),
            format!("{id}#3"),
            "spec/a#id:9f3c21a7".to_owned(),
            format!("private:{id}"),
        ] {
            assert!(Ref::parse(&ok, Place::Body).is_ok(), "{ok}");
        }
        assert_eq!(
            Ref::parse("docs/a.md", Place::Body),
            Err(RefError::NotAllowedHere {
                form: RefForm::Path,
                place: Place::Body
            })
        );
    }

    #[test]
    fn a_command_argument_takes_every_form() {
        let id = id_text(5);
        for text in [
            id.clone(),
            "9f3c21a7".to_owned(),
            "docs/a.md".to_owned(),
            format!("{id}#3"),
            "private:9f3c21a7".to_owned(),
            format!("ext:p/{id}"),
        ] {
            assert!(Ref::parse(&text, Place::Argument).is_ok(), "{text}");
        }
    }

    // --- looking up ---------------------------------------------------------

    fn at(visibility: Visibility, path: &str) -> Location {
        let git = match visibility {
            Visibility::Public => GitPlace::Tracked,
            Visibility::Private => GitPlace::Ignored,
        };
        Location::new(visibility, path.parse().unwrap(), Knowable::Known(git))
    }

    fn spec_file(
        n: u128,
        alias: Option<&str>,
        visibility: Visibility,
        path: &str,
    ) -> (FileFacts, Reading) {
        let header = Header::new(
            docid(n),
            DocKind::Spec,
            alias.map(|a| a.parse().unwrap()),
            BTreeSet::new(),
            None,
        );
        let entity = Entity::Spec(spec(n, &[900], &["PIN-A"]));
        (
            FileFacts::new(at(visibility, path), BytesHash::of(path.as_bytes())),
            Reading::Document(Document::new(header, entity).unwrap()),
        )
    }

    fn design_file(
        n: u128,
        alias: Option<&str>,
        items: Vec<(u32, crate::domain::design::WorkItem)>,
    ) -> (FileFacts, Reading) {
        let header = Header::new(
            docid(n),
            DocKind::Design,
            alias.map(|a| a.parse().unwrap()),
            BTreeSet::new(),
            None,
        );
        let entity = Entity::Design(design(n, 11, items));
        (
            FileFacts::new(
                at(Visibility::Public, &format!("design-{n}.md")),
                BytesHash::of(&n.to_le_bytes()),
            ),
            Reading::Document(Document::new(header, entity).unwrap()),
        )
    }

    fn finding_file(n: u128, visibility: Visibility) -> (FileFacts, Reading) {
        let header = Header::new(docid(n), DocKind::Finding, None, BTreeSet::new(), None);
        let entity = Entity::Finding(finding(n, "body"));
        (
            FileFacts::new(
                at(visibility, &format!("finding-{n}.md")),
                BytesHash::of(&n.to_le_bytes()),
            ),
            Reading::Document(Document::new(header, entity).unwrap()),
        )
    }

    fn corpus(files: Vec<(FileFacts, Reading)>) -> Corpus {
        Corpus::build(files, &BTreeSet::new())
    }

    fn resolve(corpus: &Corpus, text: &str) -> Resolution<Named> {
        resolve_with(
            corpus,
            &AliasHistory::new(),
            &ItemNumberHistory::new(),
            text,
        )
    }

    fn resolve_with(
        corpus: &Corpus,
        aliases: &AliasHistory,
        numbers: &ItemNumberHistory,
        text: &str,
    ) -> Resolution<Named> {
        Resolver::new(corpus, aliases, numbers).resolve(&parse(text))
    }

    fn doc(n: u128) -> Named {
        Named::Doc(docid(n))
    }

    #[test]
    fn an_id_resolves_where_a_file_has_it_and_dangles_where_none_does() {
        let c = corpus(vec![spec_file(11, None, Visibility::Public, "a.md")]);
        assert_eq!(resolve(&c, &id_text(11)), Resolution::Resolved(doc(11)));
        assert_eq!(
            resolve(&c, &id_text(12)),
            Resolution::Dangling { hint: None }
        );
    }

    #[test]
    fn a_short_id_matches_the_end_of_an_id_and_lists_candidates_when_two_end_alike() {
        let c = corpus(vec![
            spec_file(0x9f3c_21a7, None, Visibility::Public, "a.md"),
            spec_file(0x1_aaaa_bbbb, None, Visibility::Public, "b.md"),
            spec_file(0x2_aaaa_bbbb, None, Visibility::Private, "c.md"),
        ]);
        assert_eq!(
            resolve(&c, "9f3c21a7"),
            Resolution::Resolved(doc(0x9f3c_21a7))
        );
        match resolve(&c, "aaaabbbb") {
            Resolution::Ambiguous(candidates) => assert_eq!(
                candidates.iter().cloned().collect::<Vec<_>>(),
                vec![doc(0x1_aaaa_bbbb), doc(0x2_aaaa_bbbb)]
            ),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
        assert_eq!(resolve(&c, "deadbeef"), Resolution::Dangling { hint: None });
    }

    #[test]
    fn a_short_id_is_not_matched_at_the_start() {
        let c = corpus(vec![spec_file(
            0x9f3c_21a7,
            None,
            Visibility::Public,
            "a.md",
        )]);
        let start = &docid(0x9f3c_21a7).to_string().replace('-', "")[..10];
        assert_eq!(resolve(&c, start), Resolution::Dangling { hint: None });
    }

    #[test]
    fn a_bare_hexadecimal_text_is_a_short_id_and_never_an_alias() {
        // No alias is hexadecimal of that length, so a bare text is never ambiguous
        // between an alias and a short id.
        assert!(matches!(parse("cafebabe"), Ref::Short(_)));
        assert!("cafebabe".parse::<Alias>().is_err());
        assert!(matches!(parse("cafe-babe"), Ref::Alias(_)));
        assert!(matches!(parse("v20240101"), Ref::Alias(_)));

        let c = corpus(vec![
            spec_file(1, Some("cafe-babe"), Visibility::Public, "a.md"),
            spec_file(0xcafe_babe, None, Visibility::Public, "b.md"),
        ]);
        // The short id finds only the id that ends so; the alias, only its document.
        assert_eq!(
            resolve(&c, "cafebabe"),
            Resolution::Resolved(doc(0xcafe_babe))
        );
        assert_eq!(resolve(&c, "cafe-babe"), Resolution::Resolved(doc(1)));
    }

    #[test]
    fn an_uppercase_short_id_is_refused_and_not_read_as_a_path() {
        let bad = |ch| Err(RefError::Short(ValueError::ShortIdBadChar { ch }));
        for place in [Place::Argument, Place::Key, Place::Body] {
            assert_eq!(Ref::parse("9F3C21A7", place), bad('F'), "{place}");
        }
        assert_eq!(Ref::parse("CAFEBABE", Place::Argument), bad('C'));
        assert_eq!(Ref::parse(&"9F3C21A7".repeat(4), Place::Argument), bad('F'));
        // As the base of an item too.
        assert_eq!(Ref::parse("9F3C21A7#3", Place::Argument), bad('F'));
        // Outside 8 to 32 characters, or with another character, it is read as before.
        assert!(matches!(parse("9F3C21A"), Ref::Path(_)));
        assert!(matches!(parse("9F3C21A7.md"), Ref::Path(_)));
        assert!(matches!(
            parse(&format!("{}9", "9F3C21A7".repeat(4))),
            Ref::Path(_)
        ));
    }

    #[test]
    fn an_alias_two_documents_share_is_ambiguous() {
        let c = corpus(vec![
            spec_file(1, Some("spec/a"), Visibility::Public, "a.md"),
            spec_file(2, Some("spec/a"), Visibility::Public, "b.md"),
        ]);
        match resolve(&c, "spec/a") {
            Resolution::Ambiguous(candidates) => assert_eq!(
                candidates.iter().cloned().collect::<Vec<_>>(),
                vec![doc(1), doc(2)]
            ),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn a_path_is_looked_up_in_either_root() {
        let c = corpus(vec![
            spec_file(1, None, Visibility::Public, "docs/a.md"),
            spec_file(2, None, Visibility::Private, "docs/b.md"),
            spec_file(3, None, Visibility::Private, "docs/c.md"),
            spec_file(4, None, Visibility::Public, "docs/c.md"),
        ]);
        assert_eq!(resolve(&c, "docs/a.md"), Resolution::Resolved(doc(1)));
        assert_eq!(resolve(&c, "docs/b.md"), Resolution::Resolved(doc(2)));
        assert!(matches!(resolve(&c, "docs/c.md"), Resolution::Ambiguous(_)));
        assert_eq!(
            resolve(&c, "docs/none.md"),
            Resolution::Dangling { hint: None }
        );
    }

    #[test]
    fn a_renamed_alias_dangles_with_the_new_name_as_a_hint() {
        let c = corpus(vec![spec_file(
            1,
            Some("spec/new"),
            Visibility::Public,
            "a.md",
        )]);
        let mut aliases = AliasHistory::new();
        aliases.record(docid(1), "spec/old".parse().unwrap());
        aliases.record(docid(1), "spec/new".parse().unwrap());
        assert_eq!(
            resolve_with(&c, &aliases, &ItemNumberHistory::new(), "spec/old"),
            Resolution::Dangling {
                hint: Some(Rename::Alias {
                    was: "spec/old".parse().unwrap(),
                    now: "spec/new".parse().unwrap(),
                    doc: docid(1),
                })
            }
        );
        // No history: nothing to say.
        assert_eq!(resolve(&c, "spec/old"), Resolution::Dangling { hint: None });
    }

    #[test]
    fn an_alias_hint_is_given_only_when_one_document_fits() {
        let c = corpus(vec![
            spec_file(1, Some("spec/one"), Visibility::Public, "a.md"),
            spec_file(2, Some("spec/two"), Visibility::Public, "b.md"),
            spec_file(3, None, Visibility::Public, "c.md"),
        ]);
        let mut aliases = AliasHistory::new();
        aliases.record(docid(1), "spec/old".parse().unwrap());
        aliases.record(docid(2), "spec/old".parse().unwrap());
        // Two documents had it: no guess.
        let got = resolve_with(&c, &aliases, &ItemNumberHistory::new(), "spec/old");
        assert_eq!(got, Resolution::Dangling { hint: None });
        // A document with no alias now has no name to point at.
        let mut aliases = AliasHistory::new();
        aliases.record(docid(3), "spec/gone".parse().unwrap());
        let got = resolve_with(&c, &aliases, &ItemNumberHistory::new(), "spec/gone");
        assert_eq!(got, Resolution::Dangling { hint: None });
    }

    fn with_design() -> Corpus {
        corpus(vec![design_file(
            21,
            Some("design/cache"),
            vec![
                (1, identified(0x1_9f3c_21a7, "one", &[])),
                (5, identified(0x2_0000_0002, "five", &[])),
                (6, item("six, no id", &[])),
            ],
        )])
    }

    fn named_item(number: u32, id: u128) -> Named {
        Named::Item(NamedItem {
            design: docid(21),
            id: iid(id),
            number: num(number),
        })
    }

    #[test]
    fn an_item_is_found_by_number_and_by_the_end_of_its_id() {
        let c = with_design();
        let by_number = resolve(&c, "design/cache#5");
        assert_eq!(
            by_number,
            Resolution::Resolved(named_item(5, 0x2_0000_0002))
        );
        let by_id = resolve(&c, "design/cache#id:9f3c21a7");
        assert_eq!(by_id, Resolution::Resolved(named_item(1, 0x1_9f3c_21a7)));
        let by_full = resolve(&c, &format!("{}#id:00000002", id_text(21)));
        assert_eq!(by_full, Resolution::Resolved(named_item(5, 0x2_0000_0002)));
        assert_eq!(
            resolve(&c, "design/cache#id:deadbeef"),
            Resolution::Dangling { hint: None }
        );
    }

    #[test]
    fn items_of_one_design_that_end_alike_are_listed_not_chosen() {
        let c = corpus(vec![design_file(
            21,
            None,
            vec![
                (1, identified(0x1_0000_0001, "a", &[])),
                (2, identified(0x2_0000_0001, "b", &[])),
            ],
        )]);
        match resolve(&c, &format!("{}#id:00000001", id_text(21))) {
            Resolution::Ambiguous(candidates) => assert_eq!(candidates.len(), 2),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn a_renumbered_item_dangles_with_its_new_number_as_a_hint() {
        let c = with_design();
        let mut numbers = ItemNumberHistory::new();
        numbers.record(docid(21), iid(0x2_0000_0002), num(3));
        numbers.record(docid(21), iid(0x2_0000_0002), num(5));
        let got = resolve_with(&c, &AliasHistory::new(), &numbers, "design/cache#3");
        assert_eq!(
            got,
            Resolution::Dangling {
                hint: Some(Rename::ItemNumber {
                    design: docid(21),
                    was: num(3),
                    now: num(5),
                    item: iid(0x2_0000_0002),
                })
            }
        );
        assert_eq!(
            resolve(&c, "design/cache#3"),
            Resolution::Dangling { hint: None }
        );
    }

    #[test]
    fn an_item_with_no_id_and_a_base_that_is_no_design_do_not_resolve() {
        let c = corpus(vec![
            design_file(21, Some("design/cache"), vec![(6, item("six, no id", &[]))]),
            spec_file(11, Some("spec/a"), Visibility::Public, "a.md"),
        ]);
        assert_eq!(resolve(&c, "design/cache#6"), Resolution::Unresolved);
        assert_eq!(resolve(&c, "spec/a#1"), Resolution::Dangling { hint: None });
        // A base that dangles, or is ambiguous, says so for the item.
        assert_eq!(
            resolve(&c, "spec/none#1"),
            Resolution::Dangling { hint: None }
        );
    }

    #[test]
    fn a_private_reference_that_names_nothing_is_unresolved_never_dangling() {
        let c = corpus(vec![finding_file(1, Visibility::Private)]);
        let text = format!("private:{}", id_text(1));
        assert_eq!(resolve(&c, &text), Resolution::Resolved(doc(1)));
        let none = format!("private:{}", id_text(2));
        // Nothing is named: it is not told from a document that is someone else's,
        // so it is never `Dangling`, by id or by short id.
        assert_eq!(resolve(&c, &none), Resolution::Unresolved);
        assert_eq!(resolve(&c, "private:deadbeef"), Resolution::Unresolved);
        assert_eq!(
            resolve(&c, "private:00000001"),
            Resolution::Resolved(doc(1))
        );
        // The same where the corpus has no private document at all.
        let empty = corpus(vec![]);
        assert_eq!(resolve(&empty, &none), Resolution::Unresolved);
        // A plain id that names nothing still dangles: it is not a claim of privacy.
        assert_eq!(
            resolve(&c, &id_text(2)),
            Resolution::Dangling { hint: None }
        );
    }

    #[test]
    fn external_references_and_urls_cannot_be_looked_up() {
        let c = corpus(vec![]);
        assert_eq!(
            resolve(&c, &format!("ext:p/{}", id_text(1))),
            Resolution::Unresolved
        );
        assert_eq!(resolve(&c, "https://example.org/a"), Resolution::Unresolved);
    }

    // --- lints --------------------------------------------------------------

    #[test]
    fn an_item_number_in_a_document_is_fragile() {
        let c = corpus(vec![]);
        let reference = Ref::parse("spec/a#3", Place::Body).unwrap();
        assert_eq!(
            document_lints(&reference, &c),
            vec![NamingLint::FragileItemRef {
                reference: reference.clone()
            }]
        );
        let by_id = Ref::parse("spec/a#id:9f3c21a7", Place::Body).unwrap();
        assert!(document_lints(&by_id, &c).is_empty());
    }

    #[test]
    fn a_private_mark_on_a_public_document_is_stale() {
        let c = corpus(vec![
            finding_file(1, Visibility::Public),
            finding_file(2, Visibility::Private),
        ]);
        let stale = Ref::parse(&format!("private:{}", id_text(1)), Place::Body).unwrap();
        assert_eq!(
            document_lints(&stale, &c),
            vec![NamingLint::MarkStale {
                reference: stale.clone(),
                doc: docid(1)
            }]
        );
        let still = Ref::parse(&format!("private:{}", id_text(2)), Place::Body).unwrap();
        assert!(document_lints(&still, &c).is_empty());
        // A document this corpus does not have is not judged.
        let unknown = Ref::parse(&format!("private:{}", id_text(3)), Place::Body).unwrap();
        assert!(document_lints(&unknown, &c).is_empty());
        // A plain id is not the business of this lint (the leak lint reads it).
        let plain = Ref::parse(&id_text(1), Place::Body).unwrap();
        assert!(document_lints(&plain, &c).is_empty());
    }

    #[test]
    fn a_mark_is_found_on_the_base_of_an_item_too() {
        let c = corpus(vec![finding_file(1, Visibility::Public)]);
        let both = Ref::parse(&format!("private:{}#4", id_text(1)), Place::Body).unwrap();
        let lints = document_lints(&both, &c);
        assert_eq!(lints.len(), 2);
        assert!(matches!(lints[0], NamingLint::FragileItemRef { .. }));
        assert!(matches!(lints[1], NamingLint::MarkStale { .. }));
    }
}
