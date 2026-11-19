//! The conformance test of the contract of an adapter (on the page of the parent
//! module): one test, given a hook of an adapter's tests, that holds the adapter to
//! the parts of the contract a test over one tree can see.
//!
//! A hook writes a tree of its space into a directory, builds the adapter over it,
//! and says what each name of the tree comes to: there, absent, no name, a thing that
//! cannot be read, one outside the root, one past a bound, one that matches two
//! things, two names of one thing, one whose address its store does not hold, and a
//! move of a prefix. The test checks:
//!
//! - the space the adapter names, and its presets: each for that space, the same each
//!   time;
//! - that every name asked has an answer and none is an error of the reading, a name
//!   that cannot be read asked with the others;
//! - the answer of each name: there under every comparison, absent, no name (with the
//!   same reason over a directory with nothing in it), and cannot be told, or there
//!   under presence only where the hook declares so ([`Unread`]) for a thing that is
//!   there and cannot be read;
//! - that the current bytes of a name there are the bytes hashed, and that no other
//!   name has any;
//! - reproducibility over one machine: the same answers (kind, hashes, reason) asked
//!   again, each name alone and in two halves, and the same kinds and hashes over the
//!   tree written in another place;
//! - that each address given reads back, but for those the hook says its store does
//!   not hold, which read back none;
//! - one answer for a name of two things, the same hashes for two names of one thing,
//!   and the move of a prefix (the same answer again and over nothing, and a name of
//!   the space built);
//! - one name or root past each bound of bytes the hook declares, and none past an
//!   undeclared one; a name past the depth for an adapter that walks nesting;
//! - the seven headings of the module page.
//!
//! It does not check that large inputs are worked on a blocking thread, that nothing
//! else is read or written, the bound of time (the tests of the space `file` hold it
//! on their own), or that the answers do not depend on the time or the locale (it
//! runs on one machine, once). A kind the hook cannot make here (a file that opens to
//! every user, no Git, no symbolic links, or none in the space) is printed to standard
//! error as not checked, which a test run shows with `--nocapture`, and is never
//! passed: what the hook can make differs from one machine to another, so a fixed list
//! of it would pass or fail with the machine.
//!
//! The adapter is asked in one place (`ask`), the way a project asks it: through the
//! dispatch of the core over a registry of that adapter alone, by the ports the use
//! cases ask. So the asks are made one way for every adapter, and the move of a prefix
//! and reading back are answered the way the adapter's type declares.

use std::collections::BTreeSet;
use std::path::Path;

use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, ArtifactState, Comparison, ContentAddress, PrefixMove, Renamed,
    Space, SpacePrefixes, SpaceReadings, SpaceRenamed, Untold,
};
use whyshelf_core::domain::shelf::schema::BuiltinPresets;

use super::Adapter;

/// The headings of the definition of a space each adapter writes on its module page,
/// read from the one place they are written: the table on the page of the parent
/// module (the first column of the rows under `| heading | what it says |`).
fn headings() -> Vec<&'static str> {
    const PAGE: &str = include_str!("mod.rs");
    let rows = match PAGE.split_once("//! | heading | what it says |\n//! |---|---|\n") {
        Some((_, rest)) => rest,
        None => panic!("the parent module page has no table of headings"),
    };
    let headings: Vec<&str> = rows
        .lines()
        .map_while(|line| line.strip_prefix("//! | `"))
        .filter_map(|row| row.split_once('`').map(|(heading, _)| heading))
        .collect();
    assert!(!headings.is_empty(), "the table of headings has rows");
    headings
}

/// What a hook wrote, and what each of its names comes to.
pub(super) struct Tree {
    /// The adapter over the tree.
    pub(super) adapter: Adapter,
    /// Names of things that are there, whose content is read.
    pub(super) present: Vec<ArtifactName>,
    /// Names with nothing there.
    pub(super) absent: Vec<ArtifactName>,
    /// Texts that are no name of the space.
    pub(super) not_names: Vec<ArtifactName>,
    /// A name of a thing that is there and cannot be read.
    pub(super) unreadable: Option<ArtifactName>,
    /// A name that leads outside the root and the build directory, through a
    /// symbolic link.
    pub(super) outside: Option<ArtifactName>,
    /// One for each thing the adapter reads bytes of, past its bound.
    pub(super) over_bytes: Vec<OverBytes>,
    /// A name of a thing nested one past the depth the adapter walks.
    pub(super) too_deep: Option<ArtifactName>,
    /// A name that matches two things, which the rule of the space makes one answer.
    pub(super) two_things: Option<ArtifactName>,
    /// Two names of one thing.
    pub(super) spellings: Option<(ArtifactName, ArtifactName)>,
    /// Names there (among the others) whose address is of a content the store of the
    /// adapter does not hold (a file Git never added): each reads back none.
    pub(super) unstored: Vec<ArtifactName>,
    /// A move of a prefix, a name under its `from`, and that name under its `to`.
    pub(super) prefix: Option<(PrefixMove, ArtifactName, ArtifactName)>,
}

impl Tree {
    /// A tree read by `adapter` with no name yet.
    pub(super) fn of(adapter: Adapter) -> Self {
        Self {
            adapter,
            present: Vec::new(),
            absent: Vec::new(),
            not_names: Vec::new(),
            unreadable: None,
            outside: None,
            over_bytes: Vec::new(),
            too_deep: None,
            two_things: None,
            spellings: None,
            unstored: Vec::new(),
            prefix: None,
        }
    }

    /// Every name of the tree, each once.
    fn names(&self) -> Vec<ArtifactName> {
        let mut names: BTreeSet<ArtifactName> = BTreeSet::new();
        names.extend(self.present.iter().cloned());
        names.extend(self.unstored.iter().cloned());
        names.extend(self.absent.iter().cloned());
        names.extend(self.not_names.iter().cloned());
        names.extend(self.untold());
        names.extend(self.two_things.iter().cloned());
        if let Some((one, other)) = &self.spellings {
            names.extend([one.clone(), other.clone()]);
        }
        if let Some((_, name, _)) = &self.prefix {
            names.insert(name.clone());
        }
        names.into_iter().collect()
    }

    /// The names of things that are there and cannot be read: refused, or past a
    /// bound of bytes of that name. Each is told as the hook's [`Unread`] says.
    fn unread(&self) -> Vec<ArtifactName> {
        let over = self.over_bytes.iter().filter_map(|over| match over {
            OverBytes::Name { name, .. } => Some(name.clone()),
            OverBytes::Root { .. } => None,
        });
        self.unreadable.iter().cloned().chain(over).collect()
    }

    /// The names that cannot be told under any comparison: outside the root and the
    /// build directory, or nested past the depth.
    fn untold_whole(&self) -> Vec<ArtifactName> {
        self.outside.iter().chain(&self.too_deep).cloned().collect()
    }

    /// The names with no content to be told.
    fn untold(&self) -> Vec<ArtifactName> {
        let mut untold = self.unread();
        untold.extend(self.untold_whole());
        untold
    }

    /// The names of things that are there with a content.
    fn there(&self) -> Vec<ArtifactName> {
        let mut there = self.present.clone();
        there.extend(self.unstored.iter().cloned());
        there.extend(self.two_things.iter().cloned());
        if let Some((one, other)) = &self.spellings {
            there.extend([one.clone(), other.clone()]);
        }
        there
    }
}

/// A thing past a bound of bytes the adapter reads, by the bound (one of the hook's
/// [`Hook::bounds`]).
pub(super) enum OverBytes {
    /// A name in the tree whose thing is one byte past the bound: told as the hook's
    /// [`Unread`] says.
    Name {
        /// The bound.
        bound: &'static str,
        /// The name.
        name: ArtifactName,
    },
    /// A thing read for every name, not named by one (a file of settings): a root of
    /// its own where it is one byte past the bound, and names asked there, each of
    /// which cannot be told under any comparison.
    Root {
        /// The bound.
        bound: &'static str,
        /// The adapter over that root.
        adapter: Adapter,
        /// The names asked.
        names: Vec<ArtifactName>,
    },
}

impl OverBytes {
    /// The bound it is past.
    fn bound(&self) -> &'static str {
        match self {
            Self::Name { bound, .. } | Self::Root { bound, .. } => bound,
        }
    }
}

/// How an adapter tells a name whose thing is there and cannot be read (refused, or
/// past a bound of bytes of that name).
pub(super) enum Unread {
    /// It cannot be told, under any comparison.
    CannotTell,
    /// It is there under presence, and cannot be told under any other comparison
    /// (`rust-item`, as DEC-54 decides: the page of the item is listed).
    ThereForPresence,
}

/// What an adapter's tests give the conformance test.
pub(super) trait Hook {
    /// The space of the adapter.
    fn space(&self) -> &'static str;
    /// The source of the adapter's module, whose page holds the definition of its
    /// space.
    fn source(&self) -> &'static str;
    /// The bounds of bytes the adapter holds what it reads for a name to, one for
    /// each thing it reads for a name (a file, a page, a file of settings): the tree
    /// gives one [`OverBytes`] for each. The bounds of what is read back or searched
    /// for moves are held by the adapter's own tests.
    fn bounds(&self) -> Vec<&'static str>;
    /// How it tells a name whose thing is there and cannot be read.
    fn unread(&self) -> Unread;
    /// Whether the adapter walks a nesting of what it reads, and so holds it to a
    /// depth.
    fn walks_nesting(&self) -> bool;
    /// The adapter over `dir`, with nothing written there.
    fn adapter_at(&self, dir: &Path) -> Adapter;
    /// Writes the tree into `dir`, which is empty, and says what it wrote.
    async fn tree(&self, dir: &Path) -> Tree;
}

/// The comparisons every name is asked under.
fn every() -> BTreeSet<Comparison> {
    BTreeSet::from([
        Comparison::Presence,
        Comparison::Bytes,
        Comparison::Lines {
            ignore: BTreeSet::new(),
        },
    ])
}

/// The one place the conformance test asks an adapter: through the dispatch of the
/// core ([`Spaces`](whyshelf_core::app::shelf::Spaces)) over a registry of that
/// adapter alone, by the ports the use cases ask ([`ArtifactSource`], and the
/// reading back [`ContentStore`](whyshelf_core::app::shelf::ContentStore) is answered
/// by), as a project asks it.
mod ask {
    // arch-lint: allow(ports-by-the-dispatch) reason="the test asks the dispatch through the port it answers, and implements none"
    use whyshelf_core::app::shelf::{ArtifactSource, Spaces};
    use whyshelf_core::domain::shelf::artifact::{ArtifactAsks, ArtifactKey, PrefixAsks};

    use super::super::Registry;
    use super::*;

    /// The dispatch over `adapter` alone, and the one space it names.
    fn dispatch(adapter: &Adapter) -> (Spaces<Registry>, Space) {
        let spaces = match Spaces::new(Registry::of_one(adapter.clone())) {
            Ok(spaces) => spaces,
            Err(e) => panic!("the adapter cannot be dispatched to: {e}"),
        };
        let named: Vec<Space> = spaces.spaces().into_iter().collect();
        match named.as_slice() {
            [space] => (spaces, space.clone()),
            other => panic!("the adapter names one space, not {other:?}"),
        }
    }

    /// The space the adapter names, and the presets it offers.
    pub(super) fn named(adapter: &Adapter) -> (String, BuiltinPresets) {
        let (spaces, space) = dispatch(adapter);
        (space.as_str().to_owned(), spaces.presets())
    }

    pub(super) async fn resolve(adapter: &Adapter, names: &[ArtifactName]) -> SpaceReadings {
        let (spaces, space) = dispatch(adapter);
        let mut asks = ArtifactAsks::new();
        for name in names {
            for comparison in every() {
                asks.ask(space.clone(), name.clone(), comparison);
            }
        }
        match spaces.resolve(&asks).await {
            Ok(readings) => readings
                .iter()
                .map(|(_, name, reading)| (name.clone(), reading.clone()))
                .collect(),
            Err(e) => panic!("the reading of {names:?} is an error: {e:?}"),
        }
    }

    pub(super) async fn current(adapter: &Adapter, name: &ArtifactName) -> Option<Vec<u8>> {
        let (spaces, space) = dispatch(adapter);
        match spaces.current(&ArtifactKey::new(space, name.clone())).await {
            Ok(bytes) => bytes,
            Err(e) => panic!("the current bytes of {name} are an error: {e:?}"),
        }
    }

    pub(super) fn renamed(
        adapter: &Adapter,
        name: &ArtifactName,
        of: &PrefixMove,
    ) -> (SpacePrefixes, SpaceRenamed) {
        let (spaces, space) = dispatch(adapter);
        let mut asks = PrefixAsks::new();
        asks.ask(space.clone(), name.clone());
        let answers = match spaces.renamed(&asks, of) {
            Ok(answers) => answers,
            Err(e) => panic!("the move of a prefix of {name} is an error: {e:?}"),
        };
        let prefixes = match answers.prefixes_of(&space) {
            Some(prefixes) => prefixes.clone(),
            None => panic!("the space asked has no answer"),
        };
        let renamed = answers
            .renamed(&space, name)
            .map(|answer| (name.clone(), answer.clone()))
            .into_iter()
            .collect();
        (prefixes, renamed)
    }

    pub(super) async fn read_back(adapter: &Adapter, address: &ContentAddress) -> Option<Vec<u8>> {
        let (spaces, space) = dispatch(adapter);
        match spaces.read_back(&space, address).await {
            Ok(bytes) => bytes,
            Err(e) => panic!("reading {address} back is an error: {e:?}"),
        }
    }
}

/// What a reading says under every comparison: the hash, absence or why it cannot
/// be told; not the address, which is outside reproducibility.
fn told(reading: &ArtifactReading) -> Vec<ArtifactState> {
    every().iter().map(|c| reading.state(c)).collect()
}

/// The kind of a reading, without its hashes or its reason.
fn kind(reading: &ArtifactReading) -> &'static str {
    if reading.not_a_name_why().is_some() {
        return "not a name";
    }
    match reading.state(&Comparison::Bytes) {
        ArtifactState::Present(_) => "there",
        ArtifactState::Absent => "absent",
        ArtifactState::Unknown(_) => "cannot tell",
    }
}

/// Whether the reading tells, with a reason, that it cannot be told under
/// `comparison`.
fn untold_under(reading: &ArtifactReading, comparison: &Comparison) -> bool {
    matches!(
        reading.state(comparison),
        ArtifactState::Unknown(Untold::Said(_))
    )
}

/// Whether the reading tells, with a reason, that it cannot be told under any
/// comparison.
fn cannot_tell(reading: &ArtifactReading) -> bool {
    every().iter().all(|c| untold_under(reading, c))
}

/// Whether the reading tells a thing that is there and cannot be read as `how` says.
fn unread_as(reading: &ArtifactReading, how: &Unread) -> bool {
    match how {
        Unread::CannotTell => cannot_tell(reading),
        Unread::ThereForPresence => every().iter().all(|c| match c {
            Comparison::Presence => matches!(reading.state(c), ArtifactState::Present(_)),
            _ => untold_under(reading, c),
        }),
    }
}

/// Holds the adapter of `hook` to the contract.
pub(super) async fn check(hook: &impl Hook) {
    let space = match Space::new(hook.space()) {
        Ok(space) => space,
        Err(e) => panic!("the hook names no space: {e:?}"),
    };
    let mut unchecked: Vec<&str> = Vec::new();
    let dir = || match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("no directory for a tree: {e}"),
    };
    let (one, two, empty) = (dir(), dir(), dir());
    let tree = hook.tree(one.path()).await;
    let adapter = &tree.adapter;
    let names = tree.names();

    // Its space and its presets: each preset for its own space, the same each time.
    let (named, presets) = ask::named(adapter);
    assert_eq!(named, space.as_str(), "{space}: its space");
    assert!(!presets.is_empty(), "{space}: no preset");
    for (name, values) in &presets {
        assert_eq!(
            values.space.as_deref(),
            Some(space.as_str()),
            "{space}: the preset {name} is for another space"
        );
    }
    assert_eq!(presets, ask::named(adapter).1, "{space}: presets");

    // Every name is answered, a name that cannot be read beside the others, with no
    // error of the whole reading.
    let whole = ask::resolve(adapter, &names).await;
    assert_eq!(
        whole.keys().cloned().collect::<Vec<_>>(),
        names,
        "{space}: every name asked has an answer"
    );
    for name in &tree.present {
        for state in told(&whole[name]) {
            assert!(
                matches!(state, ArtifactState::Present(_)),
                "{space}: {name} is there: {state:?}"
            );
        }
    }
    for name in &tree.absent {
        for state in told(&whole[name]) {
            assert_eq!(state, ArtifactState::Absent, "{space}: {name} is absent");
        }
    }
    for name in &tree.not_names {
        assert!(
            whole[name].not_a_name_why().is_some(),
            "{space}: {name} is no name: {:?}",
            whole[name]
        );
    }
    let unread = hook.unread();
    for name in tree.unread() {
        assert!(
            unread_as(&whole[&name], &unread),
            "{space}: {name}, there and not read, is told as the hook says: {:?}",
            whole[&name]
        );
    }
    for name in tree.untold_whole() {
        assert!(
            cannot_tell(&whole[&name]),
            "{space}: {name} cannot be told: {:?}",
            whole[&name]
        );
    }

    // A text is no name by the grammar alone: the same answer over nothing at all.
    let bare = hook.adapter_at(empty.path());
    let over_nothing = ask::resolve(&bare, &tree.not_names).await;
    for name in &tree.not_names {
        assert_eq!(
            told(&over_nothing[name]),
            told(&whole[name]),
            "{space}: {name} is no name whatever is there"
        );
    }

    // The current bytes are the bytes the reading hashed, and none for a name with
    // no content.
    for name in tree.there() {
        let bytes = ask::current(adapter, &name).await;
        let Some(bytes) = bytes else {
            panic!("{space}: {name} is there and has no current bytes");
        };
        assert_eq!(
            whole[&name].state(&Comparison::Bytes),
            ArtifactState::Present(Comparison::Bytes.fingerprint(&bytes)),
            "{space}: the current bytes of {name} are the ones hashed"
        );
    }
    let no_content = tree
        .absent
        .iter()
        .chain(&tree.not_names)
        .cloned()
        .chain(tree.untold());
    for name in no_content {
        assert_eq!(
            ask::current(adapter, &name).await,
            None,
            "{space}: {name} has no current bytes"
        );
    }

    // Reproducible: again, each name alone, and in two halves, the same answers.
    let again = ask::resolve(adapter, &names).await;
    for name in &names {
        assert_eq!(
            told(&again[name]),
            told(&whole[name]),
            "{space}: {name} again"
        );
        let alone = ask::resolve(adapter, std::slice::from_ref(name)).await;
        assert_eq!(
            told(&alone[name]),
            told(&whole[name]),
            "{space}: {name} alone"
        );
    }
    let (odd, even): (Vec<_>, Vec<_>) = names
        .iter()
        .cloned()
        .enumerate()
        .partition(|(i, _)| i % 2 == 1);
    for half in [odd, even] {
        let half: Vec<ArtifactName> = half.into_iter().map(|(_, name)| name).collect();
        let found = ask::resolve(adapter, &half).await;
        for name in &half {
            assert_eq!(
                told(&found[name]),
                told(&whole[name]),
                "{space}: {name} in half"
            );
        }
    }

    // The same tree in another place: the same kinds, and the same hashes.
    let twin = hook.tree(two.path()).await;
    assert_eq!(twin.names(), names, "{space}: the hook writes one tree");
    let elsewhere = ask::resolve(&twin.adapter, &names).await;
    for name in &names {
        assert_eq!(
            kind(&elsewhere[name]),
            kind(&whole[name]),
            "{space}: {name} in another root"
        );
    }
    for name in tree.there() {
        assert_eq!(
            told(&elsewhere[&name]),
            told(&whole[&name]),
            "{space}: the content of {name} does not depend on where the tree is"
        );
    }

    // An address is read back by the adapter that gave it, while its store holds the
    // content; one of a content it does not hold reads back none.
    let mut addresses = 0;
    for name in tree.there() {
        if let Some(address) = whole[&name].address() {
            addresses += 1;
            let stored = !tree.unstored.contains(&name);
            assert_eq!(
                ask::read_back(adapter, address).await.is_some(),
                stored,
                "{space}: the address of {name} reads back while its store holds it"
            );
        }
    }
    if addresses == 0 {
        unchecked.push("reading back (no address given)");
    }

    // A name that matches two things has one answer.
    match &tree.two_things {
        Some(name) => assert!(whole[name].is_present(), "{space}: {name} is one thing"),
        None => unchecked.push("a name of two things"),
    }

    // Two names of one thing have its content.
    match &tree.spellings {
        Some((one, other)) => assert_eq!(
            told(&whole[one]),
            told(&whole[other]),
            "{space}: {one} and {other} name one thing"
        ),
        None => unchecked.push("two names of one thing"),
    }

    // A move of a prefix reads nothing, and builds a name of the space.
    match &tree.prefix {
        Some((of, name, moved)) => {
            let answer = ask::renamed(adapter, name, of);
            assert_eq!(answer.0, SpacePrefixes::Answered, "{space}: {of:?}");
            assert_eq!(
                answer.1.get(name),
                Some(&Renamed::Under(moved.clone())),
                "{space}: {name} under {of:?}"
            );
            assert_eq!(answer, ask::renamed(adapter, name, of), "{space}: again");
            assert_eq!(
                answer,
                ask::renamed(&bare, name, of),
                "{space}: over nothing"
            );
            let built = ask::resolve(adapter, std::slice::from_ref(moved)).await;
            assert!(
                built[moved].not_a_name_why().is_none(),
                "{space}: {moved} is a name of the space"
            );
        }
        None => unchecked.push("a move of a prefix"),
    }

    // Each thing read is held to a bound of bytes: one past each bound declared, and
    // none past one that is not.
    let mut declared = hook.bounds();
    declared.sort_unstable();
    assert!(
        !declared.is_empty(),
        "{space}: the hook declares no bound of bytes, and every adapter reads bytes"
    );
    let mut given: Vec<&str> = tree.over_bytes.iter().map(OverBytes::bound).collect();
    given.sort_unstable();
    assert_eq!(
        given, declared,
        "{space}: one past each bound of bytes declared"
    );
    for over in &tree.over_bytes {
        if let OverBytes::Root { adapter, names, .. } = over {
            assert!(
                !names.is_empty(),
                "{space}: a root past a bound asks no name"
            );
            let found = ask::resolve(adapter, names).await;
            for name in names {
                assert!(
                    cannot_tell(&found[name]),
                    "{space}: {name}, past a bound of what is read for every name, cannot be told: {:?}",
                    found[name]
                );
            }
        }
    }
    if hook.walks_nesting() {
        assert!(
            tree.too_deep.is_some(),
            "{space}: the adapter walks nesting, and the hook gives nothing past its depth"
        );
    } else if tree.too_deep.is_none() {
        unchecked.push("a depth (it walks no nesting)");
    }
    if tree.unreadable.is_none() {
        unchecked.push("a thing that cannot be read");
    }
    if tree.outside.is_none() {
        unchecked.push("a name outside the root");
    }

    // The definition of the space, under the seven headings.
    for heading in headings() {
        assert!(
            hook.source().contains(&format!("\n//! # {heading}\n")),
            "{space}: the module page has no heading `# {heading}`"
        );
    }

    if !unchecked.is_empty() {
        eprintln!(
            "conformance of {space}: not checked: {}",
            unchecked.join("; ")
        );
    }
}
