//! The contract of the adapter of a space: what the domain asks of every adapter,
//! what an adapter may answer, and how the core reaches one without `dyn`.
//!
//! A link names a thing in a space, and an approval pins what that thing's content
//! was. So the domain asks two things of the adapter of a space: that a name leads to
//! one thing (or to an answer that it does not), and that the same name over the same
//! content gives the same answer. Everything here says one of the two, or what may be
//! asked beside them.
//!
//! ```text
//!   SpaceAdapter                   required: its space and presets, resolve, current
//!     type Prefixes  = Answers | NoPrefixes     NamePrefixes    the move of a prefix
//!     type Suggest   = Answers | CannotSuggest  SpaceSuggest    where a gone name moved
//!     type ReadBack  = Answers | NoReadBack     SpaceReadBack   content by its address
//!     type List      = Answers | CannotList     NameList        the names a selection takes
//!
//!   the dispatch ──Visit──────► one adapter (its space and presets, a move of a prefix,
//!                                            whether it reads a selection)
//!                ──AsyncVisit─► one adapter (resolve, current, suggest, read back,
//!                                            the names a selection takes)
//! ```
//!
//! An ask an adapter may answer is declared by its type: the adapter names
//! [`Answers`] and implements the small trait of the ask, or names the not-answering
//! type of the core, whose fixed answer is given without calling the adapter. The
//! compiler holds the two together: an adapter that names [`Answers`] and does not
//! implement the trait does not build. A new ask that the adapters may answer is, here,
//! a small trait, a not-answering type, an associated type of [`SpaceAdapter`], its
//! `…Way` trait with its two impls (for [`Answers`] and for the not-answering type),
//! and the not-answering type in the sealed list; in the dispatch, a visit of the ask
//! and a method that asks it (on a port the use cases see); the answer a value of the
//! domain; and one line in every adapter that names how it answers.
//!
//! What a name is stays the adapter's (the names are opaque to the domain): the
//! domain fixes the shape of the answers ([`ArtifactReading`]) and how a content is
//! hashed ([`ArtifactReading::of_bytes`]). The promises below are rules of the
//! contract, which the adapters' own conformance test checks, not something the core
//! checks when the shelf is read.
//!
//! [`ArtifactReading`]: crate::domain::shelf::artifact::ArtifactReading
//! [`ArtifactReading::of_bytes`]: crate::domain::shelf::artifact::ArtifactReading::of_bytes

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::future::Future;

use crate::domain::nonempty::{NonBlank, TextLiteral};
use crate::domain::shelf::artifact::{
    ArtifactName, ContentAddress, NameListing, NotASelection, PrefixMove, Selection,
    SelectionCheck, SpaceAsks, SpacePrefixes, SpaceReadings, SpaceRenamed,
};
use crate::domain::shelf::ledger::{GoneName, NameSuggestions, SpaceSuggestions};
use crate::domain::shelf::schema::BuiltinPresets;

/// The adapter of one space: the three asks every adapter answers, and how it answers
/// the four it may.
///
/// The promises, whatever the names of the space are:
///
/// - **Identifying.** A name leads, by the rule of its space, to one thing in what the
///   adapter reads, or to an answer that it does not (absent, cannot tell, not a
///   name). A name that can match two things gets one answer by a fixed rule (the
///   first, or the two joined in a fixed order), or cannot be told.
/// - **Reproducible.** The same name over the same content gives the same answer (its
///   kind, the hash of its content, its reason), however often, in whatever order and
///   with whatever else it is asked; the content does not depend on where the tree
///   sits, the time or the locale. The address an answer carries, and the suggestions
///   of where a name moved, are outside this promise.
/// - **Content only.** An adapter answers what the content is, never whether it is
///   right.
///
/// Each adapter writes the definition of its space (its grammar, equal names, the one
/// thing of a name, its answers, what is compared and where it reads) on its own page.
pub trait SpaceAdapter: Send + Sync {
    /// The adapter's error: for what stops an ask as a whole, never for one name.
    type Error: Error + Send + Sync + 'static;
    /// How it answers a move of a prefix of its names: [`Answers`] (and
    /// [`NamePrefixes`]) or [`NoPrefixes`].
    type Prefixes: PrefixesWay<Self>;
    /// How it answers where a gone name moved: [`Answers`] (and [`SpaceSuggest`]) or
    /// [`CannotSuggest`].
    type Suggest: SuggestWay<Self>;
    /// How it answers the content at an address it gave: [`Answers`] (and
    /// [`SpaceReadBack`]) or [`NoReadBack`].
    type ReadBack: ReadBackWay<Self>;
    /// How it answers which names a selection takes: [`Answers`] (and [`NameList`])
    /// or [`CannotList`].
    type List: ListWay<Self>;

    /// The name of the space it reads names of: the one place a space's name comes
    /// from, the same each time. The dispatch keys the adapter by it, and a text that
    /// is no space, or a space two adapters name, is an error of the dispatch.
    fn space(&self) -> &str;

    /// The presets of links to artifacts it offers, as values, each for its own space
    /// (its `space` is [`space`](Self::space)), the same each time. A preset of
    /// another space, or of a name another adapter offers too, is an error of the
    /// dispatch.
    fn presets(&self) -> BuiltinPresets;

    /// For each name asked, one answer of the four of an
    /// [`ArtifactReading`](crate::domain::shelf::artifact::ArtifactReading): there
    /// (its hashes made with
    /// [`ArtifactReading::of_bytes`](crate::domain::shelf::artifact::ArtifactReading::of_bytes)
    /// from the bytes read, and an address only when it answers
    /// [`SpaceReadBack`]), absent, cannot tell (with why), or not a name (with why).
    /// Every name asked has an answer. A text is no name by the grammar of the space
    /// alone, with no I/O. A name whose thing is there and cannot be read, or is past a
    /// bound of what is read for it, cannot be told (or, where the adapter says so on
    /// its page, is there under presence only and cannot be told under any other
    /// comparison: an item of `rust-item` whose own page cannot be read, as its page is
    /// listed), and every other name is answered: an error
    /// is for what stops the whole ask, and then no name is answered. It reads and
    /// writes nothing else, and reads nothing outside the root of the project or the
    /// build directory it declares.
    fn resolve(
        &self,
        asks: &SpaceAsks,
    ) -> impl Future<Output = Result<SpaceReadings, Self::Error>> + Send;

    /// The bytes of `name` as they are now: the bytes [`resolve`](Self::resolve)
    /// hashes under the comparison of bytes, or none when the name is no name, has
    /// nothing there, or cannot be told.
    fn current(
        &self,
        name: &ArtifactName,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;
}

/// The move of a prefix of the names of a space, answered by its adapter: the domain
/// knows no boundary of a name and builds no name, so only the grammar of the space
/// can say where a prefix ends in a name and what the same name is under another.
pub trait NamePrefixes: SpaceAdapter {
    /// For the `names` of its space asked about, whether `from` of `of` is a prefix of
    /// its names ([`SpacePrefixes`]) and, for each name, whether it is under `from`
    /// and what the same name is under `to`
    /// ([`Renamed`](crate::domain::shelf::artifact::Renamed)). It reads nothing (no
    /// I/O): the answer is of the names alone, done on the values the grammar reads
    /// them into and never by looking for a text in a name, and a name it builds is a
    /// name of its space. An error is for an answer it could not make.
    fn renamed(
        &self,
        names: &BTreeSet<ArtifactName>,
        of: &PrefixMove,
    ) -> Result<(SpacePrefixes, SpaceRenamed), Self::Error>;
}

/// Where the gone names of a space may have moved, as its adapter can look: a search,
/// kept apart from [`SpaceAdapter::resolve`], since it may find nothing and the
/// reading answers every name.
pub trait SpaceSuggest: SpaceAdapter {
    /// For each of `gone`, names of its space with nothing behind them now, the names
    /// it may have moved to, found by what each approval pinned of it, with how each
    /// was found and whether something is there now; every name asked has an entry.
    /// What could not be looked at is a note of the name, never an error. The
    /// suggestions are never pinned, so they are outside the promise of
    /// reproducibility.
    fn suggest(
        &self,
        gone: &[GoneName],
    ) -> impl Future<Output = Result<BTreeMap<ArtifactName, NameSuggestions>, Self::Error>> + Send;
}

/// The content an adapter kept by an address it gave, read back.
pub trait SpaceReadBack: SpaceAdapter {
    /// The content at `address`, an address its [`resolve`](SpaceAdapter::resolve)
    /// gave, as its store holds it (not always the bytes hashed: the store may keep
    /// another form of the line endings), or none when its store holds no such
    /// content.
    fn read_back(
        &self,
        address: &ContentAddress,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;
}

/// The names of a space that a selection takes, as its adapter lists them: a listing
/// kept apart from [`SpaceAdapter::resolve`], which answers the names it is given,
/// since a space may have no way to list its names.
///
/// What a selection is stays the adapter's: the domain keeps its texts and never
/// reads them ([`Selection`]), so only the grammar of the space says whether a text
/// of one is a pattern of it, and which names it takes.
pub trait NameList: SpaceAdapter {
    /// Whether the selection is one of the space: every text of it read by the
    /// grammar of the space, or a [`NotASelection`] that says why not. It reads
    /// nothing (no I/O): the same answer whatever is there.
    fn select(&self, selection: &Selection) -> Result<(), NotASelection>;

    /// The names there are now that `selection` takes. Every one of them is a name
    /// that [`resolve`](SpaceAdapter::resolve) answers as there, and the same content
    /// gives the same answer. A listing past a bound of what the adapter lists is
    /// [`NameListing::Some`], never [`NameListing::All`]; one that cannot be made (the
    /// project is not where the adapter can list from) is [`NameListing::CannotList`],
    /// not an error. An error is for what stops the whole ask. A selection that
    /// [`select`](Self::select) refuses is [`NameListing::CannotList`] with why.
    fn list(
        &self,
        selection: &Selection,
    ) -> impl Future<Output = Result<NameListing, Self::Error>> + Send;
}

/// The adapter answers this ask itself, through the small trait of the ask. A type,
/// never a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answers {}

/// The adapter takes no prefix of its names: every space asked is
/// [`SpacePrefixes::NoPrefixes`], with no name answered. A type, never a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoPrefixes {}

impl NoPrefixes {
    /// The answer of a space that takes no prefix.
    pub fn answer() -> (SpacePrefixes, SpaceRenamed) {
        (SpacePrefixes::NoPrefixes, SpaceRenamed::new())
    }
}

/// The adapter has no way to look for where a name moved: every space asked is
/// [`SpaceSuggestions::CannotSuggest`]. A type, never a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannotSuggest {}

impl CannotSuggest {
    /// The answer of a space that cannot suggest.
    pub fn answer() -> SpaceSuggestions {
        SpaceSuggestions::CannotSuggest
    }
}

/// The adapter keeps nothing by address: it gives no address, and every address reads
/// back none. A type, never a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoReadBack {}

impl NoReadBack {
    /// The answer of a space that reads nothing back.
    pub fn answer() -> Option<Vec<u8>> {
        None
    }
}

/// The adapter lists no names: it reads no selection, and every listing is
/// [`NameListing::CannotList`]. A type, never a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannotList {}

impl CannotList {
    /// The reason of the fixed answers.
    const WHY: TextLiteral = TextLiteral::new("the space does not list its names");

    /// The answer of a space that lists nothing to a selection.
    pub fn selection() -> SelectionCheck {
        SelectionCheck::CannotList(NonBlank::from(Self::WHY))
    }

    /// The answer of a space that lists nothing to a listing.
    pub fn listing() -> NameListing {
        NameListing::CannotList(NonBlank::from(Self::WHY))
    }
}

/// The ways of answering are the core's: an adapter names one, and makes none.
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Answers {}
    impl Sealed for super::NoPrefixes {}
    impl Sealed for super::CannotSuggest {}
    impl Sealed for super::NoReadBack {}
    impl Sealed for super::CannotList {}
}

/// How the adapter `A` answers a move of a prefix: [`Answers`] calls its
/// [`NamePrefixes`], [`NoPrefixes`] gives its fixed answer and calls nothing.
pub trait PrefixesWay<A: SpaceAdapter + ?Sized>: sealed::Sealed {
    /// The answer of `adapter` for these names of its space and this move.
    fn renamed(
        adapter: &A,
        names: &BTreeSet<ArtifactName>,
        of: &PrefixMove,
    ) -> Result<(SpacePrefixes, SpaceRenamed), A::Error>;
}

impl<A: NamePrefixes + ?Sized> PrefixesWay<A> for Answers {
    fn renamed(
        adapter: &A,
        names: &BTreeSet<ArtifactName>,
        of: &PrefixMove,
    ) -> Result<(SpacePrefixes, SpaceRenamed), A::Error> {
        adapter.renamed(names, of)
    }
}

impl<A: SpaceAdapter + ?Sized> PrefixesWay<A> for NoPrefixes {
    fn renamed(
        _: &A,
        _: &BTreeSet<ArtifactName>,
        _: &PrefixMove,
    ) -> Result<(SpacePrefixes, SpaceRenamed), A::Error> {
        Ok(Self::answer())
    }
}

/// How the adapter `A` answers where its gone names moved: [`Answers`] calls its
/// [`SpaceSuggest`], [`CannotSuggest`] gives its fixed answer and calls nothing.
pub trait SuggestWay<A: SpaceAdapter + ?Sized>: sealed::Sealed {
    /// The answer of `adapter` for these gone names of its space.
    fn suggest(
        adapter: &A,
        gone: &[GoneName],
    ) -> impl Future<Output = Result<SpaceSuggestions, A::Error>> + Send;
}

impl<A: SpaceSuggest + ?Sized> SuggestWay<A> for Answers {
    async fn suggest(adapter: &A, gone: &[GoneName]) -> Result<SpaceSuggestions, A::Error> {
        adapter.suggest(gone).await.map(SpaceSuggestions::Suggested)
    }
}

impl<A: SpaceAdapter + ?Sized> SuggestWay<A> for CannotSuggest {
    async fn suggest(_: &A, _: &[GoneName]) -> Result<SpaceSuggestions, A::Error> {
        Ok(Self::answer())
    }
}

/// How the adapter `A` answers the content at an address: [`Answers`] calls its
/// [`SpaceReadBack`], [`NoReadBack`] gives its fixed answer and calls nothing.
pub trait ReadBackWay<A: SpaceAdapter + ?Sized>: sealed::Sealed {
    /// The answer of `adapter` for `address`.
    fn read_back(
        adapter: &A,
        address: &ContentAddress,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, A::Error>> + Send;
}

impl<A: SpaceReadBack + ?Sized> ReadBackWay<A> for Answers {
    async fn read_back(adapter: &A, address: &ContentAddress) -> Result<Option<Vec<u8>>, A::Error> {
        adapter.read_back(address).await
    }
}

impl<A: SpaceAdapter + ?Sized> ReadBackWay<A> for NoReadBack {
    async fn read_back(_: &A, _: &ContentAddress) -> Result<Option<Vec<u8>>, A::Error> {
        Ok(Self::answer())
    }
}

/// How the adapter `A` answers which names a selection takes: [`Answers`] calls its
/// [`NameList`], [`CannotList`] gives its fixed answers and calls nothing.
pub trait ListWay<A: SpaceAdapter + ?Sized>: sealed::Sealed {
    /// Whether `adapter` reads `selection`, with no I/O.
    fn select(adapter: &A, selection: &Selection) -> SelectionCheck;

    /// The names of `adapter`'s space that `selection` takes.
    fn list(
        adapter: &A,
        selection: &Selection,
    ) -> impl Future<Output = Result<NameListing, A::Error>> + Send;
}

impl<A: NameList + ?Sized> ListWay<A> for Answers {
    fn select(adapter: &A, selection: &Selection) -> SelectionCheck {
        match adapter.select(selection) {
            Ok(()) => SelectionCheck::Ok,
            Err(not) => not.into(),
        }
    }

    async fn list(adapter: &A, selection: &Selection) -> Result<NameListing, A::Error> {
        adapter.list(selection).await
    }
}

impl<A: SpaceAdapter + ?Sized> ListWay<A> for CannotList {
    fn select(_: &A, _: &Selection) -> SelectionCheck {
        Self::selection()
    }

    async fn list(_: &A, _: &Selection) -> Result<NameListing, A::Error> {
        Ok(Self::listing())
    }
}

/// One ask put to one adapter, with no I/O: its space and presets, a move of a
/// prefix, or whether it reads a selection. A registry hands it to the adapter at a
/// number, whatever its type; `E` is the registry's error, which every adapter's
/// error goes into.
pub trait Visit<E> {
    /// The answer.
    type Output;

    /// The answer of `adapter`.
    fn visit<A: SpaceAdapter>(self, adapter: &A) -> Self::Output
    where
        E: From<A::Error>;
}

/// One ask put to one adapter that reads: resolve, current, suggest, read back or the
/// names a selection takes. As [`Visit`], with the answer a future.
pub trait AsyncVisit<E>: Send {
    /// The answer.
    type Output: Send;

    /// The answer of `adapter`.
    fn visit<A: SpaceAdapter>(self, adapter: &A) -> impl Future<Output = Self::Output> + Send
    where
        E: From<A::Error>;
}
