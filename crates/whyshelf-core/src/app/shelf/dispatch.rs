//! The dispatch of the asks of the use cases to the adapters of the spaces, and the
//! answers for a space with no adapter.
//!
//! ```text
//!   use cases ──ArtifactSource, MoveSource──► Spaces ──Visit / AsyncVisit at a number──► SpaceRegistry
//!                                               │                                          └ the adapters
//!                                               └ a space no adapter names: the fixed answer of each ask
//! ```
//!
//! [`Spaces`] is built once over a [`SpaceRegistry`], a list of adapters that holds no
//! rule: each adapter names its space and its presets ([`SpaceAdapter::space`],
//! [`SpaceAdapter::presets`]), and the dispatch keys the adapters by those names and
//! refuses what does not fit ([`DispatchError`]). After that it hands each ask of a
//! space to the adapter that named it. A space no adapter names is answered here:
//!
//! | ask | answer for a space with no adapter |
//! |---|---|
//! | [`resolve`](ArtifactSource::resolve) | cannot tell, each name, with why |
//! | [`current`](ArtifactSource::current) | none |
//! | [`renamed`](ArtifactSource::renamed) | the answer of [`NoPrefixes`] |
//! | [`suggest`](MoveSource::suggest) | the answer of [`CannotSuggest`] |
//! | [`read_back`](Spaces::read_back) | the answer of [`NoReadBack`] |
//! | [`select`](ArtifactSource::select) | [`SelectionCheck::CannotList`], "no adapter offers the space" |
//! | [`list`](ArtifactSource::list) | [`NameListing::CannotList`], "no adapter offers the space" |

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::future::Future;

use crate::domain::error::ValueError;
use crate::domain::nonempty::{NonBlank, TextLiteral};
use crate::domain::shelf::artifact::{
    ArtifactAsks, ArtifactKey, ArtifactName, ArtifactReading, ArtifactReadings, ContentAddress,
    NameListing, PrefixAnswers, PrefixAsks, PrefixMove, Selection, SelectionCheck, Space,
    SpaceAsks, SpacePrefixes, SpaceProblem, SpaceReadings, SpaceRenamed,
};
use crate::domain::shelf::ledger::{GoneName, SpaceSuggestions};
use crate::domain::shelf::schema::{BuiltinPresets, PresetName};

use super::adapter::{
    AsyncVisit, CannotSuggest, ListWay, NoPrefixes, NoReadBack, PrefixesWay, ReadBackWay,
    SpaceAdapter, SuggestWay, Visit,
};
use super::ports::{ArtifactSource, MoveSource};

/// The adapters of the spaces, as a list: what [`Spaces`] dispatches to. It holds no
/// rule and names no space; it hands an ask to the adapter at a number, whatever its
/// type.
pub trait SpaceRegistry: Send + Sync {
    /// The registry's error, which the error of every adapter goes into. A reason the
    /// dispatch could not write goes into it too (a bug if it ever happens: the
    /// dispatch writes no blank reason).
    type Error: Error + Send + Sync + From<ValueError> + 'static;

    /// The answer of the adapter at `at`, numbered from 0, or none past the last. The
    /// same number is the same adapter each time.
    fn visit<V: Visit<Self::Error>>(&self, at: usize, visit: V) -> Option<V::Output>;

    /// The answer of the adapter at `at`, as [`visit`](Self::visit), for an ask that
    /// reads.
    fn visit_async<V: AsyncVisit<Self::Error>>(
        &self,
        at: usize,
        visit: V,
    ) -> impl Future<Output = Option<V::Output>> + Send;
}

/// Why the adapters of a registry cannot be dispatched to: each space has one adapter
/// and each built-in preset one name, for the adapter's own space, so that no adapter
/// silently takes the place of another.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DispatchError {
    /// An adapter names, as its space, a text that is no space.
    #[error("an adapter names no space")]
    NotASpace(#[source] SpaceProblem),
    /// Two adapters name one space.
    #[error("two adapters name the space `{0}`")]
    SpaceTwice(Space),
    /// The adapters of two spaces offer a built-in preset of one name.
    #[error("the adapters of `{first}` and `{second}` both offer a built-in preset `{preset}`")]
    PresetTwice {
        /// The name of the preset.
        preset: PresetName,
        /// The space of the adapter that offers it first, in the order of the registry.
        first: Space,
        /// The space of the other.
        second: Space,
    },
    /// An adapter offers a built-in preset that is not for its own space.
    #[error(
        "the adapter of `{space}` offers a built-in preset `{preset}` for {}",
        of.as_deref().map_or_else(|| "no space".to_owned(), |of| format!("the space `{of}`"))
    )]
    PresetOfAnotherSpace {
        /// The name of the preset.
        preset: PresetName,
        /// The space of the adapter.
        space: Space,
        /// The space the preset says it is for, if it says one.
        of: Option<String>,
    },
}

/// The asks of the use cases, each handed to the adapter of its space.
///
/// It answers [`ArtifactSource`] and [`MoveSource`], and reads content back for
/// [`ContentStore`](super::ports::ContentStore) ([`read_back`](Self::read_back)); the
/// ports keep their shape, and the use cases do not see the adapters.
#[derive(Debug, Clone)]
pub struct Spaces<R> {
    adapters: R,
    /// The number of the adapter of each space.
    at: BTreeMap<Space, usize>,
    /// The built-in presets of every adapter, each for its own space.
    presets: BuiltinPresets,
}

impl<R: SpaceRegistry> Spaces<R> {
    /// The dispatch over `adapters`: each adapter keyed by the space it names, with
    /// the presets it offers. A text that is no space, a space two adapters name, a
    /// preset of one name two adapters offer, and a preset an adapter offers for
    /// another space than its own are each an error.
    pub fn new(adapters: R) -> Result<Self, DispatchError> {
        let mut at = BTreeMap::new();
        let mut presets = BuiltinPresets::new();
        let mut owner: BTreeMap<PresetName, Space> = BTreeMap::new();
        let mut number = 0;
        while let Some((text, offered)) = adapters.visit(number, Named) {
            let space = Space::new(&text).map_err(DispatchError::NotASpace)?;
            if at.insert(space.clone(), number).is_some() {
                return Err(DispatchError::SpaceTwice(space));
            }
            for (preset, values) in offered {
                if values.space.as_deref() != Some(space.as_str()) {
                    return Err(DispatchError::PresetOfAnotherSpace {
                        preset,
                        space,
                        of: values.space,
                    });
                }
                match owner.entry(preset.clone()) {
                    Entry::Occupied(first) => {
                        return Err(DispatchError::PresetTwice {
                            preset,
                            first: first.get().clone(),
                            second: space,
                        });
                    }
                    Entry::Vacant(free) => {
                        free.insert(space.clone());
                    }
                }
                presets.insert(preset, values);
            }
            number += 1;
        }
        Ok(Self {
            adapters,
            at,
            presets,
        })
    }

    /// The answer of the adapter of `space` to `visit`, or none when no adapter names
    /// the space.
    fn ask<V: Visit<R::Error>>(&self, space: &Space, visit: V) -> Option<V::Output> {
        let at = *self.at.get(space)?;
        self.adapters.visit(at, visit)
    }

    /// The answer of the adapter of `space` to `visit`, or none when no adapter names
    /// the space.
    async fn ask_reading<V: AsyncVisit<R::Error>>(
        &self,
        space: &Space,
        visit: V,
    ) -> Option<V::Output> {
        let at = *self.at.get(space)?;
        self.adapters.visit_async(at, visit).await
    }

    /// The content of an artifact of `space` at `address`, read back by the adapter of
    /// the space: none when no adapter names the space, its adapter reads nothing back
    /// ([`NoReadBack`]), or its store holds nothing at the address; an error when the
    /// adapter could not ask its store.
    pub async fn read_back(
        &self,
        space: &Space,
        address: &ContentAddress,
    ) -> Result<Option<Vec<u8>>, R::Error> {
        match self.ask_reading(space, ReadBack { address }).await {
            Some(content) => content,
            None => Ok(NoReadBack::answer()),
        }
    }
}

impl<R: SpaceRegistry> ArtifactSource for Spaces<R> {
    type Error = R::Error;

    fn spaces(&self) -> BTreeSet<Space> {
        self.at.keys().cloned().collect()
    }

    fn presets(&self) -> BuiltinPresets {
        self.presets.clone()
    }

    async fn resolve(&self, asks: &ArtifactAsks) -> Result<ArtifactReadings, R::Error> {
        let mut readings = ArtifactReadings::new();
        for (space, names) in asks.spaces() {
            let found = match self.ask_reading(space, Resolve { asks: names }).await {
                Some(found) => found?,
                None => {
                    let why = NonBlank::new(format!("no adapter offers the space `{space}`"))?;
                    names
                        .keys()
                        .map(|name| (name.clone(), ArtifactReading::cannot_tell(why.clone())))
                        .collect()
                }
            };
            readings.extend_space(space.clone(), found);
        }
        Ok(readings)
    }

    fn renamed(&self, asks: &PrefixAsks, of: &PrefixMove) -> Result<PrefixAnswers, R::Error> {
        let mut answers = PrefixAnswers::new(of.clone());
        for (space, names) in asks.spaces() {
            let (prefixes, renamed) = match self.ask(space, Rename { names, of }) {
                Some(answer) => answer?,
                None => NoPrefixes::answer(),
            };
            answers.answer(space.clone(), prefixes, renamed);
        }
        Ok(answers)
    }

    async fn current(&self, artifact: &ArtifactKey) -> Result<Option<Vec<u8>>, R::Error> {
        let name = artifact.name();
        match self.ask_reading(artifact.space(), Current { name }).await {
            Some(bytes) => bytes,
            None => Ok(None),
        }
    }

    fn select(&self, space: &Space, selection: &Selection) -> Result<SelectionCheck, R::Error> {
        match self.ask(space, Select { selection }) {
            Some(check) => Ok(check),
            None => Ok(SelectionCheck::CannotList(NonBlank::from(NO_ADAPTER))),
        }
    }

    async fn list(&self, space: &Space, selection: &Selection) -> Result<NameListing, R::Error> {
        match self.ask_reading(space, ListNames { selection }).await {
            Some(listing) => listing,
            None => Ok(NameListing::CannotList(NonBlank::from(NO_ADAPTER))),
        }
    }
}

/// The reason of the answers of a selection and a listing for a space no adapter
/// names.
const NO_ADAPTER: TextLiteral = TextLiteral::new("no adapter offers the space");

impl<R: SpaceRegistry> MoveSource for Spaces<R> {
    type Error = R::Error;

    async fn suggest(
        &self,
        space: &Space,
        gone: &[GoneName],
    ) -> Result<SpaceSuggestions, R::Error> {
        match self.ask_reading(space, Suggest { gone }).await {
            Some(suggestions) => suggestions,
            None => Ok(CannotSuggest::answer()),
        }
    }
}

/// The space an adapter names, as written, and the presets it offers.
struct Named;

impl<E> Visit<E> for Named {
    type Output = (String, BuiltinPresets);

    fn visit<A: SpaceAdapter>(self, adapter: &A) -> Self::Output
    where
        E: From<A::Error>,
    {
        (adapter.space().to_owned(), adapter.presets())
    }
}

/// A move of a prefix, for the names of the adapter's space.
struct Rename<'a> {
    names: &'a BTreeSet<ArtifactName>,
    of: &'a PrefixMove,
}

impl<E> Visit<E> for Rename<'_> {
    type Output = Result<(SpacePrefixes, SpaceRenamed), E>;

    fn visit<A: SpaceAdapter>(self, adapter: &A) -> Self::Output
    where
        E: From<A::Error>,
    {
        A::Prefixes::renamed(adapter, self.names, self.of).map_err(E::from)
    }
}

/// The reading of the names of the adapter's space.
struct Resolve<'a> {
    asks: &'a SpaceAsks,
}

impl<E: Send> AsyncVisit<E> for Resolve<'_> {
    type Output = Result<SpaceReadings, E>;

    async fn visit<A: SpaceAdapter>(self, adapter: &A) -> Self::Output
    where
        E: From<A::Error>,
    {
        adapter.resolve(self.asks).await.map_err(E::from)
    }
}

/// The current bytes of a name of the adapter's space.
struct Current<'a> {
    name: &'a ArtifactName,
}

impl<E: Send> AsyncVisit<E> for Current<'_> {
    type Output = Result<Option<Vec<u8>>, E>;

    async fn visit<A: SpaceAdapter>(self, adapter: &A) -> Self::Output
    where
        E: From<A::Error>,
    {
        adapter.current(self.name).await.map_err(E::from)
    }
}

/// Where the gone names of the adapter's space moved.
struct Suggest<'a> {
    gone: &'a [GoneName],
}

impl<E: Send> AsyncVisit<E> for Suggest<'_> {
    type Output = Result<SpaceSuggestions, E>;

    async fn visit<A: SpaceAdapter>(self, adapter: &A) -> Self::Output
    where
        E: From<A::Error>,
    {
        A::Suggest::suggest(adapter, self.gone)
            .await
            .map_err(E::from)
    }
}

/// Whether the adapter reads a selection.
struct Select<'a> {
    selection: &'a Selection,
}

impl<E> Visit<E> for Select<'_> {
    type Output = SelectionCheck;

    fn visit<A: SpaceAdapter>(self, adapter: &A) -> Self::Output
    where
        E: From<A::Error>,
    {
        A::List::select(adapter, self.selection)
    }
}

/// The names of the adapter's space that a selection takes.
struct ListNames<'a> {
    selection: &'a Selection,
}

impl<E: Send> AsyncVisit<E> for ListNames<'_> {
    type Output = Result<NameListing, E>;

    async fn visit<A: SpaceAdapter>(self, adapter: &A) -> Self::Output
    where
        E: From<A::Error>,
    {
        A::List::list(adapter, self.selection)
            .await
            .map_err(E::from)
    }
}

/// The content at an address the adapter gave.
struct ReadBack<'a> {
    address: &'a ContentAddress,
}

impl<E: Send> AsyncVisit<E> for ReadBack<'_> {
    type Output = Result<Option<Vec<u8>>, E>;

    async fn visit<A: SpaceAdapter>(self, adapter: &A) -> Self::Output
    where
        E: From<A::Error>,
    {
        A::ReadBack::read_back(adapter, self.address)
            .await
            .map_err(E::from)
    }
}

#[cfg(test)]
mod tests {
    use crate::app::shelf::adapter::{
        Answers, CannotList, NameList, NamePrefixes, SpaceReadBack, SpaceSuggest,
    };
    use crate::domain::shelf::artifact::{
        ArtifactState, Comparison, NamePrefix, NotASelection, Renamed, Untold,
    };
    use crate::domain::shelf::ledger::NameSuggestions;
    use crate::domain::shelf::schema::{PresetValues, Propagation};

    use super::super::fake_tests::{FakeError, block_on};
    use super::*;

    fn space(text: &str) -> Space {
        Space::new(text).unwrap()
    }

    fn name(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    fn preset(text: &str) -> PresetName {
        PresetName::new(text).unwrap()
    }

    fn address() -> ContentAddress {
        ContentAddress::from_validated("a".repeat(40)).unwrap()
    }

    fn moving(from: &str, to: &str) -> PrefixMove {
        PrefixMove::new(NamePrefix::new(from).unwrap(), NamePrefix::new(to).unwrap()).unwrap()
    }

    /// The presets of a fake: each preset's name, and the space it says it is for.
    fn presets_of(offered: &[(&str, Option<&str>)]) -> BuiltinPresets {
        offered
            .iter()
            .map(|(text, of)| {
                let values = PresetValues {
                    space: of.map(str::to_owned),
                    compare: Some("bytes".to_owned()),
                    ignore: None,
                    means: Some(Propagation::Depends),
                };
                (preset(text), values)
            })
            .collect()
    }

    /// Every name there, with its own text as its content.
    fn there(asks: &SpaceAsks) -> SpaceReadings {
        asks.iter()
            .map(|(name, comparisons)| {
                let reading =
                    ArtifactReading::of_bytes(name.as_str().as_bytes(), comparisons, None);
                (name.clone(), reading)
            })
            .collect()
    }

    /// The text a selection of the fakes is refused for.
    const NOT_A_PATTERN: &str = "!";

    fn selecting(include: &str) -> Selection {
        Selection::new(vec![include.to_owned()], vec![]).unwrap()
    }

    /// An adapter that answers every ask: a name is there with its text as content,
    /// every name is outside a prefix, no name moved, every address reads back, and a
    /// selection takes the name "a.rs" (but for [`NOT_A_PATTERN`], which no selection is).
    struct Answering {
        space: &'static str,
        presets: Vec<(&'static str, Option<&'static str>)>,
    }

    impl SpaceAdapter for Answering {
        type Error = FakeError;
        type Prefixes = Answers;
        type Suggest = Answers;
        type ReadBack = Answers;
        type List = Answers;

        fn space(&self) -> &str {
            self.space
        }

        fn presets(&self) -> BuiltinPresets {
            presets_of(&self.presets)
        }

        async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, FakeError> {
            Ok(there(asks))
        }

        async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, FakeError> {
            Ok(Some(name.as_str().as_bytes().to_vec()))
        }
    }

    impl NamePrefixes for Answering {
        fn renamed(
            &self,
            names: &BTreeSet<ArtifactName>,
            _: &PrefixMove,
        ) -> Result<(SpacePrefixes, SpaceRenamed), FakeError> {
            let renamed = names
                .iter()
                .map(|name| (name.clone(), Renamed::Outside))
                .collect();
            Ok((SpacePrefixes::Answered, renamed))
        }
    }

    impl SpaceSuggest for Answering {
        async fn suggest(
            &self,
            _: &[GoneName],
        ) -> Result<BTreeMap<ArtifactName, NameSuggestions>, FakeError> {
            Ok(BTreeMap::new())
        }
    }

    impl SpaceReadBack for Answering {
        async fn read_back(&self, _: &ContentAddress) -> Result<Option<Vec<u8>>, FakeError> {
            Ok(Some(b"kept".to_vec()))
        }
    }

    impl NameList for Answering {
        fn select(&self, selection: &Selection) -> Result<(), NotASelection> {
            match selection.include().iter().any(|text| text == NOT_A_PATTERN) {
                true => Err(NotASelection::new(
                    NonBlank::new("a bang is no pattern").unwrap(),
                )),
                false => Ok(()),
            }
        }

        async fn list(&self, selection: &Selection) -> Result<NameListing, FakeError> {
            Ok(match self.select(selection) {
                Ok(()) => NameListing::All(BTreeSet::from([name("a.rs")])),
                Err(not) => NameListing::CannotList(not.why().clone()),
            })
        }
    }

    /// An adapter that names the not-answering type of each ask it may answer, and
    /// still has a way to answer each, which must never be called.
    struct Silent;

    impl SpaceAdapter for Silent {
        type Error = FakeError;
        type Prefixes = NoPrefixes;
        type Suggest = CannotSuggest;
        type ReadBack = NoReadBack;
        type List = CannotList;

        fn space(&self) -> &str {
            "quiet"
        }

        fn presets(&self) -> BuiltinPresets {
            BuiltinPresets::new()
        }

        async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, FakeError> {
            Ok(there(asks))
        }

        async fn current(&self, _: &ArtifactName) -> Result<Option<Vec<u8>>, FakeError> {
            Ok(None)
        }
    }

    impl NamePrefixes for Silent {
        fn renamed(
            &self,
            _: &BTreeSet<ArtifactName>,
            _: &PrefixMove,
        ) -> Result<(SpacePrefixes, SpaceRenamed), FakeError> {
            panic!("an adapter that names NoPrefixes is asked for a move of a prefix")
        }
    }

    impl SpaceSuggest for Silent {
        async fn suggest(
            &self,
            _: &[GoneName],
        ) -> Result<BTreeMap<ArtifactName, NameSuggestions>, FakeError> {
            panic!("an adapter that names CannotSuggest is asked to suggest")
        }
    }

    impl SpaceReadBack for Silent {
        async fn read_back(&self, _: &ContentAddress) -> Result<Option<Vec<u8>>, FakeError> {
            panic!("an adapter that names NoReadBack is asked to read back")
        }
    }

    impl NameList for Silent {
        fn select(&self, _: &Selection) -> Result<(), NotASelection> {
            panic!("an adapter that names CannotList is asked whether it reads a selection")
        }

        async fn list(&self, _: &Selection) -> Result<NameListing, FakeError> {
            panic!("an adapter that names CannotList is asked to list")
        }
    }

    /// One adapter of the fake registry.
    enum Fake {
        Answering(Answering),
        Silent(Silent),
    }

    /// Why the fake registry failed: an adapter's error, or a reason of the dispatch.
    #[derive(Debug, thiserror::Error)]
    enum Fault {
        #[error(transparent)]
        Adapter(#[from] FakeError),
        #[error(transparent)]
        Reason(#[from] ValueError),
    }

    /// The fake registry: the adapters in a list, and the two matches.
    struct Registry(Vec<Fake>);

    impl SpaceRegistry for Registry {
        type Error = Fault;

        fn visit<V: Visit<Fault>>(&self, at: usize, visit: V) -> Option<V::Output> {
            Some(match self.0.get(at)? {
                Fake::Answering(adapter) => visit.visit(adapter),
                Fake::Silent(adapter) => visit.visit(adapter),
            })
        }

        async fn visit_async<V: AsyncVisit<Fault>>(
            &self,
            at: usize,
            visit: V,
        ) -> Option<V::Output> {
            Some(match self.0.get(at)? {
                Fake::Answering(adapter) => visit.visit(adapter).await,
                Fake::Silent(adapter) => visit.visit(adapter).await,
            })
        }
    }

    fn answering(space: &'static str, presets: &[(&'static str, Option<&'static str>)]) -> Fake {
        Fake::Answering(Answering {
            space,
            presets: presets.to_vec(),
        })
    }

    fn dispatch(adapters: Vec<Fake>) -> Spaces<Registry> {
        Spaces::new(Registry(adapters)).unwrap()
    }

    fn refused(adapters: Vec<Fake>) -> DispatchError {
        match Spaces::new(Registry(adapters)) {
            Ok(_) => panic!("the registry is dispatched to"),
            Err(e) => e,
        }
    }

    #[test]
    fn the_spaces_and_presets_are_the_ones_the_adapters_name() {
        let spaces = dispatch(vec![
            answering("file", &[("file", Some("file")), ("code", Some("file"))]),
            answering("rust-test", &[("test", Some("rust-test"))]),
            Fake::Silent(Silent),
        ]);
        assert_eq!(
            spaces.spaces(),
            BTreeSet::from([space("file"), space("quiet"), space("rust-test")])
        );
        assert_eq!(
            spaces.presets(),
            presets_of(&[
                ("code", Some("file")),
                ("file", Some("file")),
                ("test", Some("rust-test"))
            ])
        );
    }

    #[test]
    fn a_space_with_no_adapter_has_the_fixed_answer_of_each_ask() {
        let spaces = dispatch(vec![answering("file", &[])]);
        let other = space("other");

        let mut asks = ArtifactAsks::new();
        asks.ask(space("file"), name("a.rs"), Comparison::Bytes);
        asks.ask(other.clone(), name("a.rs"), Comparison::Bytes);
        let found = block_on(spaces.resolve(&asks)).unwrap();
        assert!(
            found
                .get(&space("file"), &name("a.rs"))
                .unwrap()
                .is_present()
        );
        // The same name in another space is another artifact, and no adapter names
        // that space.
        assert!(matches!(
            found.get(&other, &name("a.rs")).unwrap().state(&Comparison::Bytes),
            ArtifactState::Unknown(Untold::Said(why)) if why.as_str().contains("`other`")
        ));

        let key = ArtifactKey::new(other.clone(), name("a.rs"));
        assert_eq!(block_on(spaces.current(&key)).unwrap(), None);
        assert_eq!(
            block_on(spaces.suggest(&other, &[])).unwrap(),
            SpaceSuggestions::CannotSuggest
        );
        assert_eq!(
            block_on(spaces.read_back(&other, &address())).unwrap(),
            None
        );
        let why = NonBlank::new("no adapter offers the space").unwrap();
        assert_eq!(
            spaces.select(&other, &selecting("*.rs")).unwrap(),
            SelectionCheck::CannotList(why.clone())
        );
        assert_eq!(
            block_on(spaces.list(&other, &selecting("*.rs"))).unwrap(),
            NameListing::CannotList(why)
        );

        let mut asks = PrefixAsks::new();
        asks.ask(other.clone(), name("src/a/x.rs"));
        let answers = spaces.renamed(&asks, &moving("src/a", "src/b")).unwrap();
        assert_eq!(
            answers.prefixes_of(&other),
            Some(&SpacePrefixes::NoPrefixes)
        );
        assert_eq!(answers.renamed(&other, &name("src/a/x.rs")), None);
    }

    #[test]
    fn an_adapter_that_answers_an_ask_is_asked_it() {
        let spaces = dispatch(vec![answering("file", &[])]);
        let file = space("file");
        let key = ArtifactKey::new(file.clone(), name("a.rs"));
        assert_eq!(
            block_on(spaces.current(&key)).unwrap().as_deref(),
            Some(&b"a.rs"[..])
        );
        assert_eq!(
            block_on(spaces.suggest(&file, &[])).unwrap(),
            SpaceSuggestions::Suggested(BTreeMap::new())
        );
        assert_eq!(
            block_on(spaces.read_back(&file, &address())).unwrap(),
            Some(b"kept".to_vec())
        );
        assert_eq!(
            spaces.select(&file, &selecting("*.rs")).unwrap(),
            SelectionCheck::Ok
        );
        assert_eq!(
            spaces.select(&file, &selecting(NOT_A_PATTERN)).unwrap(),
            SelectionCheck::NotASelection(NonBlank::new("a bang is no pattern").unwrap())
        );
        assert_eq!(
            block_on(spaces.list(&file, &selecting("*.rs"))).unwrap(),
            NameListing::All(BTreeSet::from([name("a.rs")]))
        );
    }

    #[test]
    fn an_adapter_that_names_a_not_answering_type_is_not_called_for_that_ask() {
        let spaces = dispatch(vec![Fake::Silent(Silent)]);
        let quiet = space("quiet");
        let mut asks = PrefixAsks::new();
        asks.ask(quiet.clone(), name("a"));
        let answers = spaces.renamed(&asks, &moving("a", "b")).unwrap();
        assert_eq!(
            answers.prefixes_of(&quiet),
            Some(&SpacePrefixes::NoPrefixes)
        );
        assert_eq!(answers.renamed(&quiet, &name("a")), None);
        assert_eq!(
            block_on(spaces.suggest(&quiet, &[])).unwrap(),
            SpaceSuggestions::CannotSuggest
        );
        assert_eq!(
            block_on(spaces.read_back(&quiet, &address())).unwrap(),
            None
        );
        // The fixed answers of the not-answering type, with no call of the adapter.
        assert_eq!(
            spaces.select(&quiet, &selecting("*.rs")).unwrap(),
            CannotList::selection()
        );
        assert_eq!(
            block_on(spaces.list(&quiet, &selecting("*.rs"))).unwrap(),
            CannotList::listing()
        );
        // The asks every adapter answers still reach it.
        let mut asks = ArtifactAsks::new();
        asks.ask(quiet.clone(), name("a"), Comparison::Bytes);
        let found = block_on(spaces.resolve(&asks)).unwrap();
        assert!(found.get(&quiet, &name("a")).unwrap().is_present());
    }

    #[test]
    fn every_space_asked_for_a_move_of_a_prefix_is_answered_and_no_other() {
        let spaces = dispatch(vec![answering("file", &[]), Fake::Silent(Silent)]);
        let mut asks = PrefixAsks::new();
        asks.ask(space("file"), name("src/a/x.rs"));
        asks.ask(space("other"), name("src/a/x.rs"));
        asks.ask(space("quiet"), name("src/a/x.rs"));
        let answers = spaces.renamed(&asks, &moving("src/a", "src/b")).unwrap();
        assert_eq!(answers.of(), &moving("src/a", "src/b"));
        assert_eq!(
            answers.prefixes_of(&space("file")),
            Some(&SpacePrefixes::Answered)
        );
        assert_eq!(
            answers.renamed(&space("file"), &name("src/a/x.rs")),
            Some(&Renamed::Outside)
        );
        assert_eq!(
            answers.prefixes_of(&space("other")),
            Some(&SpacePrefixes::NoPrefixes)
        );
        assert_eq!(
            answers.prefixes_of(&space("quiet")),
            Some(&SpacePrefixes::NoPrefixes)
        );
        // A space not asked has no answer.
        assert_eq!(answers.prefixes_of(&space("js-test")), None);
    }

    #[test]
    fn two_adapters_that_name_one_space_are_refused() {
        assert_eq!(
            refused(vec![answering("file", &[]), answering("file", &[])]),
            DispatchError::SpaceTwice(space("file"))
        );
    }

    #[test]
    fn two_adapters_that_offer_a_built_in_preset_of_one_name_are_refused() {
        assert_eq!(
            refused(vec![
                answering("file", &[("test", Some("file"))]),
                answering("rust-test", &[("test", Some("rust-test"))]),
            ]),
            DispatchError::PresetTwice {
                preset: preset("test"),
                first: space("file"),
                second: space("rust-test"),
            }
        );
    }

    #[test]
    fn a_built_in_preset_for_another_space_than_the_adapters_own_is_refused() {
        assert_eq!(
            refused(vec![answering("file", &[("test", Some("rust-test"))])]),
            DispatchError::PresetOfAnotherSpace {
                preset: preset("test"),
                space: space("file"),
                of: Some("rust-test".to_owned()),
            }
        );
        assert_eq!(
            refused(vec![answering("file", &[("test", None)])]),
            DispatchError::PresetOfAnotherSpace {
                preset: preset("test"),
                space: space("file"),
                of: None,
            }
        );
    }

    #[test]
    fn an_adapter_that_names_no_space_is_refused() {
        assert!(matches!(
            refused(vec![answering("No Space", &[])]),
            DispatchError::NotASpace(problem) if problem.found == "No Space"
        ));
    }
}
