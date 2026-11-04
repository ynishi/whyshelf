//! An in-memory implementation of every port, for tests.
//!
//! One type, `FakePorts`, implements all the ports and the [`Ports`] bundle. State
//! sits behind a `Mutex` because the ports take `&self`. Every method is an
//! immediately-ready future (an `async fn` with no `.await`), so the tiny
//! [`block_on`] below is enough to drive a use case end to end without a runtime.
//!
//! Files are not text that a parser reads: a test puts a parsed raw value together
//! with a text, and the text decides the hashes. Changing the text changes the
//! file; changing only the parsed value does not exist.

// The module is declared inline so that the architecture lint, which looks for
// `#[cfg(test)]` modules, knows this is test code (it may use `expect`).
pub(crate) use in_memory::*;

#[cfg(test)]
mod in_memory {
    use std::collections::{BTreeMap, BTreeSet};
    use std::future::Future;
    use std::pin::pin;
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::task::{Context, Poll, Wake, Waker};

    use crate::domain::actor::{MeasuredAt, Timestamp};
    use crate::domain::approval::TestOutcome;
    use crate::domain::design::{ChosenOption, Crux, RejectedOption, WorkItem};
    use crate::domain::event::{Event, StoreKind};
    use crate::domain::finding::{EvidenceLabel, Reproducibility};
    use crate::domain::hash::ContentHash;
    use crate::domain::id::{EntityId, ItemNumber, test_support::eid};
    use crate::domain::location::{CodeLocation, RelPath};
    use crate::domain::nonempty::{NonBlank, NonEmptyVec};
    use crate::domain::pin::{HypId, Hypothesis, PinContract, PinId};
    use crate::domain::reference::Ref;
    use crate::domain::revision::RevisionId;
    use crate::domain::state::UnparsedReason;

    use crate::app::config::{Config, Paths, PinPattern, TestCommand, TestSettings};
    use crate::app::ports::{
        AppendOutcome, CacheEntry, ClockPort, CodeScanPort, CreateOutcome, CurrentPort, ExportDoc,
        ExportFile, ExportKind, ExportParts, FileStat, IdPort, IndexCachePort, Mtime, Ports,
        RevisionPort, ScaffoldPort, SecretHit, SecretScanPort, StorePort, Template, TestRunPort,
        WorktreeRoot,
    };
    use crate::app::raw::{
        Candidate, NormalizedText, RawDesign, RawDoc, RawFinding, RawFindingSource, RawRevision,
        RawSpec,
    };

    /// Drive a future to completion on the current thread. The fakes never wait, so
    /// one poll is normally enough; the loop is for a future that yields once.
    pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
        struct Noop;
        impl Wake for Noop {
            fn wake(self: Arc<Self>) {}
        }
        let waker = Waker::from(Arc::new(Noop));
        let mut cx = Context::from_waker(&waker);
        let mut future = pin!(future);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
        }
    }

    /// The only error the fakes produce, on request.
    #[derive(Debug, thiserror::Error)]
    #[error("fake failure: {0}")]
    pub(crate) struct FakeError(pub(crate) &'static str);

    #[derive(Debug, Clone)]
    #[allow(clippy::large_enum_variant)]
    enum Parsed {
        Doc(RawDoc),
        Broken(Vec<UnparsedReason>),
    }

    #[derive(Debug, Clone)]
    struct FakeFile {
        text: String,
        mtime: u64,
        parsed: Parsed,
    }

    #[derive(Debug)]
    struct State {
        files: BTreeMap<RelPath, FakeFile>,
        stamp: u64,
        events: BTreeMap<StoreKind, Vec<Event>>,
        objects: BTreeMap<(StoreKind, ContentHash), NormalizedText>,
        exports: BTreeMap<(StoreKind, ExportKind), (String, ContentHash)>,
        export_writes: usize,
        fail_exports: bool,
        revision: RevisionId,
        clean: bool,
        integrated: BTreeSet<RevisionId>,
        label: Option<String>,
        pins: BTreeMap<PinId, Vec<CodeLocation>>,
        tests: TestOutcome,
        tests_ran_at: Vec<RevisionId>,
        cache: BTreeMap<RelPath, CacheEntry>,
        cache_rebuilds: usize,
        clock: u32,
        ids: u128,
    }

    /// All the ports, in memory.
    #[derive(Debug)]
    pub(crate) struct FakePorts {
        state: Mutex<State>,
    }

    fn rev(text: &str) -> RevisionId {
        RevisionId::from_validated(text.to_owned()).expect("a test revision")
    }

    pub(crate) fn path(text: &str) -> RelPath {
        text.parse().expect("a test path")
    }

    fn nb(text: &str) -> NonBlank {
        NonBlank::new(text).expect("a test text")
    }

    impl FakePorts {
        pub(crate) fn new() -> Self {
            Self {
                state: Mutex::new(State {
                    files: BTreeMap::new(),
                    stamp: 0,
                    events: BTreeMap::new(),
                    objects: BTreeMap::new(),
                    exports: BTreeMap::new(),
                    export_writes: 0,
                    fail_exports: false,
                    revision: rev("rev-1"),
                    clean: true,
                    integrated: BTreeSet::new(),
                    label: Some("main".to_owned()),
                    pins: BTreeMap::new(),
                    tests: TestOutcome::Passed,
                    tests_ran_at: Vec::new(),
                    cache: BTreeMap::new(),
                    cache_rebuilds: 0,
                    clock: 0,
                    ids: 0,
                }),
            }
        }

        fn lock(&self) -> MutexGuard<'_, State> {
            self.state.lock().expect("the fake's lock")
        }

        fn put(&self, at: &str, text: String, parsed: Parsed) {
            let mut s = self.lock();
            s.stamp += 1;
            let mtime = s.stamp;
            s.files.insert(
                path(at),
                FakeFile {
                    text,
                    mtime,
                    parsed,
                },
            );
        }

        /// A Finding that needs no revision (a usage observation).
        pub(crate) fn put_finding(&self, at: &str, title: &str, body: &str) {
            let raw = RawFinding::new(
                nb(title),
                RawFindingSource::UsageObservation {
                    context: nb("steps"),
                    reproducibility: Reproducibility::Reproducible,
                },
                EvidenceLabel::Measured,
                body.to_owned(),
            );
            self.put(
                at,
                format!("finding\n{title}\n{body}\n"),
                Parsed::Doc(RawDoc::Finding(raw)),
            );
        }

        /// Change only the text of a file (a supplementary section), keeping what it
        /// parses to.
        pub(crate) fn put_finding_text_only(&self, at: &str, extra: &str) {
            let mut s = self.lock();
            s.stamp += 1;
            let mtime = s.stamp;
            if let Some(file) = s.files.get_mut(&path(at)) {
                file.text.push_str(extra);
                file.text.push('\n');
                file.mtime = mtime;
            }
        }

        /// A measurement, which names a revision that the port must accept.
        pub(crate) fn put_measured(&self, at: &str, title: &str, revision: &str) {
            let raw = RawFinding::new(
                nb(title),
                RawFindingSource::MeasuredData {
                    measured_at: MeasuredAt::from_validated("2026-11-05T09:00:00+09:00".to_owned())
                        .expect("a test time"),
                    revision: RawRevision::new(revision).expect("a test revision"),
                    environment: nb("laptop"),
                },
                EvidenceLabel::Measured,
                "numbers".to_owned(),
            );
            self.put(
                at,
                format!("measured\n{title}\n{revision}\n"),
                Parsed::Doc(RawDoc::Finding(raw)),
            );
        }

        /// A Spec with the given grounds (as written), pins and one hypothesis.
        pub(crate) fn put_spec(&self, at: &str, grounds: &[&str], contract: &str, pins: &[&str]) {
            let grounds_ref: Vec<Ref> = grounds
                .iter()
                .map(|g| g.parse().expect("a test reference"))
                .collect();
            let pin_map: BTreeMap<PinId, PinContract> = pins
                .iter()
                .map(|p| {
                    (
                        p.parse().expect("a test pin"),
                        PinContract::new(format!("{p} holds")).expect("a test sentence"),
                    )
                })
                .collect();
            let hyps: BTreeMap<HypId, Hypothesis> = BTreeMap::from([(
                "HYP-ZERO".parse().expect("a test hypothesis"),
                Hypothesis::new("reruns", "0").expect("a test hypothesis"),
            )]);
            let raw = RawSpec::new(
                NonEmptyVec::from_vec(grounds_ref).expect("grounds"),
                nb(contract),
                pin_map,
                hyps,
            );
            self.put(
                at,
                format!(
                    "spec\n{}\n{contract}\n{}\n",
                    grounds.join(","),
                    pins.join(",")
                ),
                Parsed::Doc(RawDoc::Spec(raw)),
            );
        }

        /// A Design for a Spec with items `(number, summary, pins)`.
        pub(crate) fn put_design(&self, at: &str, spec: &str, items: &[(u32, &str, &[&str])]) {
            let item_map: BTreeMap<ItemNumber, WorkItem> = items
                .iter()
                .map(|(n, summary, pins)| {
                    (
                        ItemNumber::new(*n).expect("a test number"),
                        WorkItem::new(
                            nb(summary),
                            pins.iter()
                                .map(|p| p.parse().expect("a test pin"))
                                .collect(),
                        ),
                    )
                })
                .collect();
            let raw = RawDesign::new(
                spec.parse().expect("a test reference"),
                test_crux(),
                item_map,
            );
            let text = format!(
                "design\n{spec}\n{}\n",
                items
                    .iter()
                    .map(|(n, s, p)| format!("{n}:{s}:{}", p.join("+")))
                    .collect::<Vec<_>>()
                    .join(";")
            );
            self.put(at, text, Parsed::Doc(RawDoc::Design(raw)));
        }

        /// A file that does not parse.
        pub(crate) fn put_broken(&self, at: &str, text: &str) {
            self.put(
                at,
                text.to_owned(),
                Parsed::Broken(vec![UnparsedReason::MergeConflictMarker]),
            );
        }

        pub(crate) fn remove_file(&self, at: &str) {
            self.lock().files.remove(&path(at));
        }

        pub(crate) fn rename_file(&self, from: &str, to: &str) {
            let mut s = self.lock();
            if let Some(mut file) = s.files.remove(&path(from)) {
                s.stamp += 1;
                file.mtime = s.stamp;
                s.files.insert(path(to), file);
            }
        }

        pub(crate) fn set_revision(&self, id: &str) {
            self.lock().revision = rev(id);
        }

        pub(crate) fn set_clean(&self, clean: bool) {
            self.lock().clean = clean;
        }

        pub(crate) fn integrate(&self, id: &str) {
            self.lock().integrated.insert(rev(id));
        }

        pub(crate) fn set_pins(&self, pins: &[&str]) {
            let mut s = self.lock();
            s.pins = pins
                .iter()
                .map(|p| {
                    (
                        p.parse().expect("a test pin"),
                        vec![CodeLocation::new(path("tests/t.rs"), 3).expect("a test location")],
                    )
                })
                .collect();
        }

        pub(crate) fn set_tests(&self, outcome: TestOutcome) {
            self.lock().tests = outcome;
        }

        pub(crate) fn tests_ran_at(&self) -> Vec<RevisionId> {
            self.lock().tests_ran_at.clone()
        }

        pub(crate) fn fail_exports(&self, fail: bool) {
            self.lock().fail_exports = fail;
        }

        pub(crate) fn events_in(&self, store: StoreKind) -> Vec<Event> {
            self.lock().events.get(&store).cloned().unwrap_or_default()
        }

        pub(crate) fn object_count(&self, store: StoreKind) -> usize {
            self.lock()
                .objects
                .keys()
                .filter(|(s, _)| *s == store)
                .count()
        }

        pub(crate) fn export_writes(&self) -> usize {
            self.lock().export_writes
        }

        pub(crate) fn cache_rebuilds(&self) -> usize {
            self.lock().cache_rebuilds
        }

        pub(crate) fn cache_len(&self) -> usize {
            self.lock().cache.len()
        }

        pub(crate) fn edit_export(&self, store: StoreKind, kind: ExportKind, text: &str) {
            if let Some(entry) = self.lock().exports.get_mut(&(store, kind)) {
                entry.0 = text.to_owned();
            }
        }

        pub(crate) fn has_file(&self, at: &str) -> bool {
            self.lock().files.contains_key(&path(at))
        }
    }

    fn test_crux() -> Crux {
        Crux::new(
            nb("which way"),
            ChosenOption::new(
                "opt-a".parse().expect("an option"),
                nb("do a"),
                nb("simplest"),
            ),
            NonEmptyVec::new(
                RejectedOption::new(
                    "opt-b".parse().expect("an option"),
                    nb("do b"),
                    nb("too slow"),
                ),
                Vec::new(),
            ),
        )
        .expect("a test crux")
    }

    fn render(doc: &ExportDoc<'_>) -> String {
        format!("{doc:?}")
    }

    fn stat_of(at: &RelPath, file: &FakeFile) -> FileStat {
        FileStat::new(
            at.clone(),
            Mtime::new(file.mtime),
            u64::try_from(file.text.len()).unwrap_or(u64::MAX),
        )
    }

    impl RevisionPort for FakePorts {
        type Error = FakeError;

        async fn validate(&self, text: &RawRevision) -> Result<Option<RevisionId>, FakeError> {
            Ok(text
                .as_str()
                .starts_with("rev-")
                .then(|| rev(text.as_str())))
        }

        async fn current(&self) -> Result<RevisionId, FakeError> {
            Ok(self.lock().revision.clone())
        }

        async fn is_clean(&self) -> Result<bool, FakeError> {
            Ok(self.lock().clean)
        }

        async fn is_integrated(&self, revision: &RevisionId) -> Result<bool, FakeError> {
            Ok(self.lock().integrated.contains(revision))
        }

        async fn label(&self) -> Result<Option<String>, FakeError> {
            Ok(self.lock().label.clone())
        }

        async fn worktree(&self) -> Result<WorktreeRoot, FakeError> {
            Ok(WorktreeRoot::new("/work/demo").expect("a test root"))
        }
    }

    impl StorePort for FakePorts {
        type Error = FakeError;

        async fn append(
            &self,
            scope: StoreKind,
            event: &Event,
        ) -> Result<AppendOutcome, FakeError> {
            let mut s = self.lock();
            let events = s.events.entry(scope).or_default();
            if events.iter().any(|e| e.id() == event.id()) {
                return Ok(AppendOutcome::AlreadyPresent);
            }
            events.push(event.clone());
            Ok(AppendOutcome::Written)
        }

        async fn events(&self, scope: StoreKind) -> Result<Vec<Event>, FakeError> {
            Ok(self.lock().events.get(&scope).cloned().unwrap_or_default())
        }

        async fn put_object(
            &self,
            scope: StoreKind,
            raw: &ContentHash,
            text: &NormalizedText,
        ) -> Result<AppendOutcome, FakeError> {
            let mut s = self.lock();
            if s.objects.contains_key(&(scope, *raw)) {
                return Ok(AppendOutcome::AlreadyPresent);
            }
            s.objects.insert((scope, *raw), text.clone());
            Ok(AppendOutcome::Written)
        }

        async fn get_object(
            &self,
            scope: StoreKind,
            raw: &ContentHash,
        ) -> Result<Option<NormalizedText>, FakeError> {
            Ok(self.lock().objects.get(&(scope, *raw)).cloned())
        }

        async fn purge_object(
            &self,
            scope: StoreKind,
            raw: &ContentHash,
        ) -> Result<bool, FakeError> {
            Ok(self.lock().objects.remove(&(scope, *raw)).is_some())
        }

        async fn write_export(
            &self,
            scope: StoreKind,
            doc: &ExportDoc<'_>,
        ) -> Result<(), FakeError> {
            let mut s = self.lock();
            if s.fail_exports {
                return Err(FakeError("export refused"));
            }
            s.export_writes += 1;
            let body = render(doc);
            let marker = ContentHash::export(body.as_bytes());
            s.exports.insert((scope, doc.kind()), (body, marker));
            Ok(())
        }

        async fn inspect_export(
            &self,
            scope: StoreKind,
            doc: &ExportDoc<'_>,
        ) -> Result<ExportFile, FakeError> {
            let s = self.lock();
            Ok(match s.exports.get(&(scope, doc.kind())) {
                None => ExportFile::Absent,
                // The marker is the hash of what was written; an edit changes the
                // body but not the marker.
                Some((body, marker)) => ExportFile::Marked(ExportParts::new(
                    *marker,
                    body.clone().into_bytes(),
                    render(doc).into_bytes(),
                )),
            })
        }
    }

    impl CurrentPort for FakePorts {
        type Error = FakeError;

        async fn list(&self, roots: &[RelPath]) -> Result<Vec<FileStat>, FakeError> {
            let s = self.lock();
            Ok(s.files
                .iter()
                .filter(|(p, _)| {
                    roots.iter().any(|r| {
                        p.as_str() == r.as_str()
                            || p.as_str().starts_with(&format!("{}/", r.as_str()))
                    })
                })
                .map(|(p, f)| stat_of(p, f))
                .collect())
        }

        async fn read(&self, at: &RelPath) -> Result<Option<Candidate>, FakeError> {
            let s = self.lock();
            Ok(s.files.get(at).map(|file| {
                let text = NormalizedText::new(file.text.clone());
                match &file.parsed {
                    Parsed::Doc(doc) => Candidate::parsed(at.clone(), text, doc.clone()),
                    Parsed::Broken(reasons) => Candidate::unparsed(
                        at.clone(),
                        text,
                        NonEmptyVec::from_vec(reasons.clone()).expect("a reason"),
                    ),
                }
            }))
        }
    }

    impl ScaffoldPort for FakePorts {
        type Error = FakeError;

        async fn create_new(
            &self,
            at: &RelPath,
            template: &Template,
        ) -> Result<CreateOutcome, FakeError> {
            if self.lock().files.contains_key(at) {
                return Ok(CreateOutcome::AlreadyExists);
            }
            match template {
                Template::Finding { title, .. } => {
                    self.put_finding(at.as_str(), title.as_str(), "todo");
                }
                Template::Spec { grounds } => {
                    let grounds: Vec<String> = grounds.iter().map(ToString::to_string).collect();
                    let refs: Vec<&str> = grounds.iter().map(String::as_str).collect();
                    self.put_spec(at.as_str(), &refs, "todo", &[]);
                }
                Template::Design { spec } => {
                    self.put_design(at.as_str(), &spec.to_string(), &[]);
                }
            }
            Ok(CreateOutcome::Created)
        }
    }

    impl CodeScanPort for FakePorts {
        type Error = FakeError;

        async fn pins_at(
            &self,
            _revision: &RevisionId,
            _scan_dirs: &[RelPath],
            _pattern: &PinPattern,
        ) -> Result<BTreeMap<PinId, Vec<CodeLocation>>, FakeError> {
            Ok(self.lock().pins.clone())
        }
    }

    impl TestRunPort for FakePorts {
        type Error = FakeError;

        async fn run(
            &self,
            revision: &RevisionId,
            _command: &TestCommand,
        ) -> Result<TestOutcome, FakeError> {
            let mut s = self.lock();
            s.tests_ran_at.push(revision.clone());
            Ok(s.tests)
        }
    }

    impl IndexCachePort for FakePorts {
        type Error = FakeError;

        async fn load(&self) -> Result<Vec<CacheEntry>, FakeError> {
            Ok(self.lock().cache.values().cloned().collect())
        }

        async fn upsert(&self, entries: &[CacheEntry]) -> Result<(), FakeError> {
            let mut s = self.lock();
            for e in entries {
                s.cache.insert(e.path().clone(), e.clone());
            }
            Ok(())
        }

        async fn remove(&self, paths: &[RelPath]) -> Result<(), FakeError> {
            let mut s = self.lock();
            for p in paths {
                s.cache.remove(p);
            }
            Ok(())
        }

        async fn rebuild(&self, entries: &[CacheEntry]) -> Result<(), FakeError> {
            let mut s = self.lock();
            s.cache = entries
                .iter()
                .map(|e| (e.path().clone(), e.clone()))
                .collect();
            s.cache_rebuilds += 1;
            Ok(())
        }

        async fn search_fts(
            &self,
            text: &NonBlank,
            limit: usize,
        ) -> Result<Vec<RelPath>, FakeError> {
            let needle = text.as_str().trim().to_lowercase();
            Ok(self
                .lock()
                .cache
                .values()
                .filter(|e| e.body().to_lowercase().contains(&needle))
                .map(|e| e.path().clone())
                .take(limit)
                .collect())
        }
    }

    impl SecretScanPort for FakePorts {
        type Error = FakeError;

        async fn scan(&self, text: &NormalizedText) -> Result<Vec<SecretHit>, FakeError> {
            Ok(text
                .as_str()
                .lines()
                .enumerate()
                .filter(|(_, line)| line.contains("AKIA-SECRET"))
                .filter_map(|(at, _)| {
                    let line = u32::try_from(at + 1)
                        .ok()
                        .and_then(std::num::NonZeroU32::new)?;
                    Some(SecretHit::new(nb("fake-key"), line))
                })
                .collect())
        }
    }

    impl ClockPort for FakePorts {
        type Error = FakeError;

        async fn now(&self) -> Result<Timestamp, FakeError> {
            let mut s = self.lock();
            s.clock += 1;
            let (m, sec) = (s.clock / 60 % 60, s.clock % 60);
            Ok(
                Timestamp::from_validated(format!("2026-11-05T10:{m:02}:{sec:02}+09:00"))
                    .expect("a test time"),
            )
        }
    }

    impl IdPort for FakePorts {
        type Error = FakeError;

        async fn new_entity_id(&self) -> Result<EntityId, FakeError> {
            let mut s = self.lock();
            s.ids += 1;
            Ok(eid(s.ids))
        }
    }

    impl Ports for FakePorts {
        type Revision = FakePorts;
        type Store = FakePorts;
        type Current = FakePorts;
        type Scaffold = FakePorts;
        type CodeScan = FakePorts;
        type TestRun = FakePorts;
        type Cache = FakePorts;
        type Secret = FakePorts;
        type Clock = FakePorts;
        type Id = FakePorts;

        fn revision(&self) -> &FakePorts {
            self
        }
        fn store(&self) -> &FakePorts {
            self
        }
        fn current(&self) -> &FakePorts {
            self
        }
        fn scaffold(&self) -> &FakePorts {
            self
        }
        fn code_scan(&self) -> &FakePorts {
            self
        }
        fn test_run(&self) -> &FakePorts {
            self
        }
        fn cache(&self) -> &FakePorts {
            self
        }
        fn secret(&self) -> &FakePorts {
            self
        }
        fn clock(&self) -> &FakePorts {
            self
        }
        fn id(&self) -> &FakePorts {
            self
        }
    }

    /// The configuration the tests use: documents under `docs/`, tests under `tests/`,
    /// new documents in the repo store, `alice` as the default approver.
    pub(crate) fn test_config() -> Config {
        use crate::domain::actor::Actor;
        use crate::domain::tag::TagVocabulary;
        let paths = Paths::new(
            path("docs/finding"),
            path("docs/spec"),
            path("docs/design"),
            vec![path("tests")],
            vec![path("docs")],
        );
        Config::new(
            "demo".parse().expect("a project name"),
            paths,
            TestSettings::new(
                TestCommand::new("cargo test").expect("a command"),
                PinPattern::new("@pin").expect("a pattern"),
            ),
        )
        .with_default_store(StoreKind::Repo)
        .with_user(Actor::new("alice").expect("a name"))
        .with_vocabulary(TagVocabulary::new(BTreeMap::from([(
            "area".to_owned(),
            BTreeSet::from(["auth".to_owned(), "build".to_owned()]),
        )])))
    }
}
