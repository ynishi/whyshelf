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
pub(crate) use corpus_fake::*;
pub(crate) use in_memory::*;

#[cfg(test)]
mod in_memory {
    use std::collections::{BTreeMap, BTreeSet};
    use std::future::Future;
    use std::pin::pin;
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::task::{Context, Poll, Wake, Waker};

    use crate::domain::actor::{MeasuredAt, Timestamp};
    use crate::domain::approval::{TestOutcome, TestsRun};
    use crate::domain::design::{ChosenOption, Crux, RejectedOption, WorkItem};
    use crate::domain::event::{DocKind, Event, Recorded, StoreKind};
    use crate::domain::finding::{EvidenceLabel, Reproducibility};
    use crate::domain::front::{FileClassification, Guess};
    use crate::domain::hash::ContentHash;
    use crate::domain::id::{EntityId, ItemNumber, test_support::eid};
    use crate::domain::location::{CodeLocation, RelPath, TestFileSet};
    use crate::domain::nonempty::{NonBlank, NonEmptyVec};
    use crate::domain::pin::{HypId, Hypothesis, MalformedPin, PinContract, PinId};
    use crate::domain::reference::{Reach, Ref};
    use crate::domain::revision::{IntegrationFact, RevisionId, UnknownIntegration};
    use crate::domain::state::UnparsedReason;

    use crate::app::config::{Config, Paths, PinSearch, TestCommand, TestSettings, TestTimeout};
    use crate::app::ports::{
        AppendOutcome, CacheEntry, ClockPort, CodeScanPort, CreateOutcome, CurrentPort, ExportDoc,
        ExportFile, ExportKind, ExportParts, FileStat, IdPort, IndexCachePort, Listing, Mtime,
        PathProblem, PinScan, Ports, RevisionPort, ScaffoldPort, SecretHit, SecretScanPort,
        StorePort, Template, TestRunPort, UnreadablePath, WorktreeRoot,
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
        /// Whether the listing cannot read the file's time.
        mtime_unknown: bool,
        parsed: Parsed,
        classification: FileClassification,
        guess: Option<Guess>,
    }

    #[derive(Debug)]
    struct State {
        files: BTreeMap<RelPath, FakeFile>,
        /// Paths the listing finds and cannot name.
        unreadable_paths: Vec<UnreadablePath>,
        /// The text of each file a scaffold created, as it wrote it.
        created: BTreeMap<RelPath, String>,
        stamp: u64,
        events: BTreeMap<StoreKind, Vec<Recorded>>,
        objects: BTreeMap<(StoreKind, ContentHash), NormalizedText>,
        exports: BTreeMap<(StoreKind, ExportKind), (String, ContentHash)>,
        export_writes: usize,
        fail_exports: bool,
        revision: RevisionId,
        clean: bool,
        integrated: BTreeSet<RevisionId>,
        cannot_tell: Option<UnknownIntegration>,
        label: Option<String>,
        pins: BTreeMap<PinId, Vec<CodeLocation>>,
        malformed: Vec<MalformedPin>,
        tests: TestOutcome,
        tests_time_out: bool,
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
                    unreadable_paths: Vec::new(),
                    created: BTreeMap::new(),
                    stamp: 0,
                    events: BTreeMap::new(),
                    objects: BTreeMap::new(),
                    exports: BTreeMap::new(),
                    export_writes: 0,
                    fail_exports: false,
                    revision: rev("rev-1"),
                    clean: true,
                    integrated: BTreeSet::new(),
                    cannot_tell: None,
                    label: Some("main".to_owned()),
                    pins: BTreeMap::new(),
                    malformed: Vec::new(),
                    tests: TestOutcome::Passed,
                    tests_time_out: false,
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
                    mtime_unknown: false,
                    parsed,
                    classification: FileClassification::default(),
                    guess: None,
                },
            );
        }

        /// Make the listing find a path it cannot name.
        pub(crate) fn add_unreadable_path(&self, shown: &str, problem: PathProblem) {
            self.lock()
                .unreadable_paths
                .push(UnreadablePath::new(shown.to_owned(), problem));
        }

        /// Make the listing unable to read the time of a file.
        pub(crate) fn forget_mtime(&self, at: &str) {
            if let Some(file) = self.lock().files.get_mut(&path(at)) {
                file.mtime_unknown = true;
            }
        }

        /// What the keys of a file say about its kind, when the file does not parse.
        pub(crate) fn set_file_guess(&self, at: &str, guess: Guess) {
            if let Some(file) = self.lock().files.get_mut(&path(at)) {
                file.guess = Some(guess);
            }
        }

        /// Give a file classification keys in its front matter.
        pub(crate) fn set_file_classification(&self, at: &str, keys: FileClassification) {
            if let Some(file) = self.lock().files.get_mut(&path(at)) {
                file.classification = keys;
            }
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

        /// A clone that lacks the baseline or the commit: what is not integrated is
        /// not known to be either.
        pub(crate) fn cannot_tell_integration(&self, reason: Option<UnknownIntegration>) {
            self.lock().cannot_tell = reason;
        }

        pub(crate) fn set_malformed(&self, tags: &[(&str, u32, &str)]) {
            self.lock().malformed = tags
                .iter()
                .map(|(file, line, text)| {
                    MalformedPin::new(
                        CodeLocation::new(path(file), *line).expect("a test location"),
                        nb(text),
                    )
                })
                .collect();
        }

        pub(crate) fn set_tests_time_out(&self, time_out: bool) {
            self.lock().tests_time_out = time_out;
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

        /// What a clone has: the same files and the repo store, and no private store.
        pub(crate) fn drop_private_store(&self) {
            let mut s = self.lock();
            s.events.remove(&StoreKind::Private);
            s.objects
                .retain(|(store, _), _| *store != StoreKind::Private);
            s.exports
                .retain(|(store, _), _| *store != StoreKind::Private);
        }

        /// The events of a store, without their times.
        pub(crate) fn events_in(&self, store: StoreKind) -> Vec<Event> {
            self.recorded_in(store)
                .into_iter()
                .map(Recorded::into_event)
                .collect()
        }

        /// The events of a store with the time of each record.
        pub(crate) fn recorded_in(&self, store: StoreKind) -> Vec<Recorded> {
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
            if file.mtime_unknown {
                Mtime::Unknown
            } else {
                Mtime::new(file.mtime)
            },
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

        async fn current(&self) -> Result<Option<RevisionId>, FakeError> {
            Ok(Some(self.lock().revision.clone()))
        }

        async fn is_clean(&self) -> Result<bool, FakeError> {
            Ok(self.lock().clean)
        }

        async fn integration(&self, revision: &RevisionId) -> Result<IntegrationFact, FakeError> {
            let s = self.lock();
            Ok(if s.integrated.contains(revision) {
                IntegrationFact::Integrated
            } else if let Some(reason) = s.cannot_tell {
                IntegrationFact::Unknown { reason }
            } else {
                IntegrationFact::NotIntegrated
            })
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

        /// A record is the pair of the identifier and the time, as a store on disk
        /// keeps it: the same pair is already present, the same identifier at
        /// another time is a further record of the one event.
        async fn append(
            &self,
            scope: StoreKind,
            event: &Event,
            at: &Timestamp,
        ) -> Result<AppendOutcome, FakeError> {
            let mut s = self.lock();
            let events = s.events.entry(scope).or_default();
            let record = Recorded::new(event.clone(), at.clone());
            match events.iter_mut().find(|e| e.event().id() == event.id()) {
                None => events.push(record),
                Some(held) => {
                    if held.at().iter().any(|held_at| held_at == at) {
                        return Ok(AppendOutcome::AlreadyPresent);
                    }
                    held.merge(record)
                        .map_err(|_| FakeError("one identifier, two different events"))?;
                }
            }
            Ok(AppendOutcome::Written)
        }

        async fn events(&self, scope: StoreKind) -> Result<Vec<Recorded>, FakeError> {
            Ok(self.lock().events.get(&scope).cloned().unwrap_or_default())
        }

        async fn private_reach(&self) -> Result<Reach, FakeError> {
            // Like a directory, the private store is there once it was written to.
            Ok(if self.lock().events.contains_key(&StoreKind::Private) {
                Reach::Available
            } else {
                Reach::Absent
            })
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

        async fn list(&self, roots: &[RelPath]) -> Result<Listing, FakeError> {
            let s = self.lock();
            let files = s
                .files
                .iter()
                .filter(|(p, _)| {
                    roots.iter().any(|r| {
                        p.as_str() == r.as_str()
                            || p.as_str().starts_with(&format!("{}/", r.as_str()))
                    })
                })
                .map(|(p, f)| stat_of(p, f))
                .collect();
            Ok(Listing::new(files, s.unreadable_paths.clone()))
        }

        async fn read(
            &self,
            at: &RelPath,
            _kind: Option<DocKind>,
        ) -> Result<Option<Candidate>, FakeError> {
            let s = self.lock();
            Ok(s.files.get(at).map(|file| {
                let text = NormalizedText::new(file.text.clone());
                let candidate = match &file.parsed {
                    Parsed::Doc(doc) => Candidate::parsed(at.clone(), text, doc.clone())
                        .with_guess(Guess::One(doc.kind())),
                    Parsed::Broken(reasons) => Candidate::unparsed(
                        at.clone(),
                        text,
                        NonEmptyVec::from_vec(reasons.clone()).expect("a reason"),
                        None,
                    ),
                };
                let candidate = match file.guess {
                    Some(guess) => candidate.with_guess(guess),
                    None => candidate,
                };
                candidate.with_classification(file.classification.clone())
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
            let mut s = self.lock();
            let text = s.files.get(at).map(|f| f.text.clone()).unwrap_or_default();
            s.created.insert(at.clone(), text);
            Ok(CreateOutcome::Created)
        }

        async fn remove_created(
            &self,
            at: &RelPath,
            _template: &Template,
        ) -> Result<bool, FakeError> {
            let mut s = self.lock();
            let untouched = match (s.created.get(at), s.files.get(at)) {
                (Some(wrote), Some(file)) => *wrote == file.text,
                _ => false,
            };
            if untouched {
                s.files.remove(at);
                s.created.remove(at);
            }
            Ok(untouched)
        }
    }

    impl CodeScanPort for FakePorts {
        type Error = FakeError;

        async fn pins_at(
            &self,
            _revision: &RevisionId,
            _scan_dirs: &[RelPath],
            _files: &TestFileSet,
            _search: &PinSearch,
        ) -> Result<PinScan, FakeError> {
            let s = self.lock();
            Ok(PinScan {
                pins: s.pins.clone(),
                malformed: s.malformed.clone(),
            })
        }
    }

    impl TestRunPort for FakePorts {
        type Error = FakeError;

        async fn run(
            &self,
            revision: &RevisionId,
            _command: &TestCommand,
            timeout: TestTimeout,
        ) -> Result<TestsRun, FakeError> {
            let mut s = self.lock();
            s.tests_ran_at.push(revision.clone());
            Ok(if s.tests_time_out {
                TestsRun::TimedOut {
                    after: timeout.get(),
                }
            } else {
                TestsRun::Finished(s.tests)
            })
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
            TestSettings::new(TestCommand::new("cargo test").expect("a command")),
        )
        .with_default_store(StoreKind::Repo)
        .with_user(Actor::new("alice").expect("a name"))
        .with_vocabulary(TagVocabulary::new(BTreeMap::from([(
            "area".to_owned(),
            BTreeSet::from(["auth".to_owned(), "build".to_owned()]),
        )])))
    }
}

/// The fake of the ports of the corpus and the ledgers ([`CorpusPorts`]).
///
/// Declared inline for the same reason as the module above.
#[cfg(test)]
mod corpus_fake {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Mutex, MutexGuard};

    use super::in_memory::{FakeError, FakePorts};

    use crate::app::commit::record::Scanned;
    use crate::app::ports::{
        ActorSource, Author, BaselineAnswer, BaselineQuery, ConfirmPort, ConfirmPrompt, Confirmed,
        CorpusPorts, CorpusRead, CorruptRecord, DocumentListing, DocumentSource, FileBytes,
        FileWriter, KeySource, LedgerAppend, LedgerContents, LedgerRead, ObjectPut, ObjectRead,
        PathProblem, PortName, Put, RevisionFactsPort, UnreadablePath, WorkTree, Write,
    };
    use crate::app::raw::RawDocument;
    use crate::domain::boundary::PrivateRoot;
    use crate::domain::corpus::{FileFacts, GitPlace, Location, Visibility};
    use crate::domain::hash::{BytesHash, TextHash};
    use crate::domain::integration::{BaselineFacts, Fingerprinting, UnknownWhy};
    use crate::domain::judge::Judge;
    use crate::domain::judge::test_support::judge;
    use crate::domain::judgement::JudgementKind;
    use crate::domain::keyring::PinKeyring;
    use crate::domain::knowable::Knowable;
    use crate::domain::location::RelPath;
    use crate::domain::publication::ConfirmDigest;
    use crate::domain::revision::Revision;
    use crate::domain::stored::StoredJudgement;

    /// How the fake person answers a question.
    #[derive(Debug, Clone, Copy)]
    pub(crate) enum Reply {
        /// The same answer to every question.
        Always(Confirmed),
        /// Types back this digest: yes to a question about it, no to any other.
        TypesBack(ConfirmDigest),
    }

    #[derive(Debug, Clone)]
    struct FakeFile {
        bytes: Vec<u8>,
        links: u32,
        regular: bool,
        group_blocks: bool,
        owner_blocks: bool,
    }

    type FileKey = (Visibility, RelPath);

    #[derive(Debug)]
    struct CorpusState {
        documents: Vec<(FileFacts, RawDocument)>,
        unreadable: Vec<UnreadablePath>,
        private_root: PrivateRoot,
        ledgers: BTreeMap<Visibility, Vec<StoredJudgement>>,
        corrupt: BTreeMap<Visibility, Vec<CorruptRecord>>,
        objects: BTreeMap<(Visibility, TextHash), String>,
        purged: BTreeSet<(Visibility, TextHash)>,
        actor: Judge,
        keys: Knowable<PinKeyring>,
        reply: Reply,
        asked: Vec<ConfirmPrompt>,
        files: BTreeMap<FileKey, FakeFile>,
        race: Option<(FileKey, Vec<u8>)>,
        baseline: BaselineAnswer,
        fingerprinting: Fingerprinting,
        work_tree: Knowable<WorkTree>,
        authors: BTreeMap<Revision, Author>,
        places: BTreeMap<FileKey, GitPlace>,
        failing: BTreeSet<PortName>,
    }

    /// The ports of the corpus and the ledgers, in memory.
    ///
    /// - The ledgers are lists of records per side; an append of a record already
    ///   there (the same canonical bytes) is success, and one of the same identifier
    ///   with other bytes is kept beside the first. Appending a purge also takes the
    ///   text out of the objects of the same side and remembers the hash as purged
    ///   there, as [`LedgerAppend::append`] says.
    /// - The objects are a 2P-set per side: a purged hash is never stored, and
    ///   [`merge_objects_from`](Self::merge_objects_from) reads a branch that stored
    ///   it before the purge as gone.
    /// - The file writer keeps bytes per `(root, path)`, and can be made to find a
    ///   link count, an irregular file, a group it cannot keep, or a writer that
    ///   changes the file between the comparison and the replacement.
    #[derive(Debug)]
    pub(crate) struct FakeCorpus {
        state: Mutex<CorpusState>,
        shared: FakePorts,
    }

    fn key(location: &Location) -> FileKey {
        (location.visibility(), location.path().clone())
    }

    impl FakeCorpus {
        pub(crate) fn new() -> Self {
            Self {
                state: Mutex::new(CorpusState {
                    documents: Vec::new(),
                    unreadable: Vec::new(),
                    private_root: PrivateRoot::Present,
                    ledgers: BTreeMap::new(),
                    corrupt: BTreeMap::new(),
                    objects: BTreeMap::new(),
                    purged: BTreeSet::new(),
                    actor: judge("ada"),
                    keys: Knowable::Known(PinKeyring::empty()),
                    reply: Reply::Always(Confirmed::Unavailable),
                    asked: Vec::new(),
                    files: BTreeMap::new(),
                    race: None,
                    baseline: BaselineAnswer::new(
                        BaselineFacts::new(Knowable::Known(false), Knowable::Known(false)),
                        UnknownWhy::Shallow,
                    ),
                    fingerprinting: Fingerprinting::NothingToFingerprint,
                    work_tree: Knowable::Known(WorkTree::Clean),
                    authors: BTreeMap::new(),
                    places: BTreeMap::new(),
                    failing: BTreeSet::new(),
                }),
                shared: FakePorts::new(),
            }
        }

        fn lock(&self) -> MutexGuard<'_, CorpusState> {
            self.state.lock().expect("the fake's lock")
        }

        /// Make every call of this port fail from now on.
        pub(crate) fn fail(&self, port: PortName) {
            self.lock().failing.insert(port);
        }

        fn check(&self, port: PortName) -> Result<(), FakeError> {
            if self.lock().failing.contains(&port) {
                return Err(FakeError("the port was made to fail"));
            }
            Ok(())
        }

        // --- documents

        pub(crate) fn add_document(&self, facts: FileFacts, raw: RawDocument) {
            self.lock().documents.push((facts, raw));
        }

        /// Make the listing find a path it cannot name.
        pub(crate) fn add_unreadable_path(&self, shown: &str, problem: PathProblem) {
            self.lock()
                .unreadable
                .push(UnreadablePath::new(shown.to_owned(), problem));
        }

        /// What the person's settings and the disk say of the private root.
        pub(crate) fn set_private_root(&self, root: PrivateRoot) {
            self.lock().private_root = root;
        }

        // --- ledgers

        /// Put a record in a ledger as it would be found there, with no check.
        pub(crate) fn seed_record(&self, side: Visibility, record: impl Into<StoredJudgement>) {
            self.lock()
                .ledgers
                .entry(side)
                .or_default()
                .push(record.into());
        }

        pub(crate) fn seed_corrupt(&self, side: Visibility, record: CorruptRecord) {
            self.lock().corrupt.entry(side).or_default().push(record);
        }

        /// The records of a ledger, in the order written.
        pub(crate) fn records(&self, side: Visibility) -> Vec<StoredJudgement> {
            self.lock().ledgers.get(&side).cloned().unwrap_or_default()
        }

        // --- objects

        /// Mark a hash as purged on one side and take its text out of that side's
        /// store, as a purge record in that side's ledger does.
        pub(crate) fn purge(&self, side: Visibility, text: TextHash) {
            self.lock().purge(side, text);
        }

        pub(crate) fn has_object(&self, side: Visibility, text: TextHash) -> bool {
            self.lock().objects.contains_key(&(side, text))
        }

        /// Read in what another branch holds: the union of the objects and of the
        /// purges, then everything purged is gone.
        pub(crate) fn merge_objects_from(&self, other: &FakeCorpus) {
            let (objects, purged) = {
                let o = other.lock();
                (o.objects.clone(), o.purged.clone())
            };
            let mut s = self.lock();
            s.objects.extend(objects);
            s.purged.extend(purged);
            let purged = s.purged.clone();
            s.objects.retain(|held, _| !purged.contains(held));
        }

        // --- who is acting, keys

        pub(crate) fn set_actor(&self, actor: Judge) {
            self.lock().actor = actor;
        }

        pub(crate) fn set_keys(&self, keys: Knowable<PinKeyring>) {
            self.lock().keys = keys;
        }

        // --- the person

        pub(crate) fn set_reply(&self, reply: Reply) {
            self.lock().reply = reply;
        }

        pub(crate) fn asked(&self) -> Vec<ConfirmPrompt> {
            self.lock().asked.clone()
        }

        // --- files

        pub(crate) fn put_file(&self, root: Visibility, at: &RelPath, bytes: &[u8]) {
            self.lock().files.insert(
                (root, at.clone()),
                FakeFile {
                    bytes: bytes.to_vec(),
                    links: 1,
                    regular: true,
                    group_blocks: false,
                    owner_blocks: false,
                },
            );
        }

        pub(crate) fn file(&self, root: Visibility, at: &RelPath) -> Option<Vec<u8>> {
            self.lock()
                .files
                .get(&(root, at.clone()))
                .map(|f| f.bytes.clone())
        }

        pub(crate) fn set_links(&self, root: Visibility, at: &RelPath, links: u32) {
            if let Some(f) = self.lock().files.get_mut(&(root, at.clone())) {
                f.links = links;
            }
        }

        pub(crate) fn make_irregular(&self, root: Visibility, at: &RelPath) {
            if let Some(f) = self.lock().files.get_mut(&(root, at.clone())) {
                f.regular = false;
            }
        }

        pub(crate) fn make_group_unkeepable(&self, root: Visibility, at: &RelPath) {
            if let Some(f) = self.lock().files.get_mut(&(root, at.clone())) {
                f.group_blocks = true;
            }
        }

        pub(crate) fn make_owner_unkeepable(&self, root: Visibility, at: &RelPath) {
            if let Some(f) = self.lock().files.get_mut(&(root, at.clone())) {
                f.owner_blocks = true;
            }
        }

        /// Another editor writes `bytes` to the file after the next write or removal of
        /// it has compared it and before it replaces it.
        pub(crate) fn race_on_next_write(&self, root: Visibility, at: &RelPath, bytes: &[u8]) {
            self.lock().race = Some(((root, at.clone()), bytes.to_vec()));
        }

        // --- version control

        pub(crate) fn set_baseline(&self, answer: BaselineAnswer) {
            self.lock().baseline = answer;
        }

        pub(crate) fn set_fingerprinting(&self, fingerprinting: Fingerprinting) {
            self.lock().fingerprinting = fingerprinting;
        }

        pub(crate) fn set_work_tree(&self, work_tree: Knowable<WorkTree>) {
            self.lock().work_tree = work_tree;
        }

        pub(crate) fn set_author(&self, at: Revision, author: Author) {
            self.lock().authors.insert(at, author);
        }

        pub(crate) fn set_place(&self, root: Visibility, at: &RelPath, place: GitPlace) {
            self.lock().places.insert((root, at.clone()), place);
        }

        /// The comparison and the replacement of one write: the file is compared with
        /// `expect`, the race (if any) happens, and the file is compared again.
        fn guarded(&self, at: &Location, expect: BytesHash) -> Result<Guard, Write> {
            let mut s = self.lock();
            let k = key(at);
            let file = s.files.get(&k).ok_or(Write::ChangedUnderfoot)?;
            if !file.regular {
                return Err(Write::NotRegularFile);
            }
            if BytesHash::of(&file.bytes) != expect {
                return Err(Write::ChangedUnderfoot);
            }
            if file.links > 1 {
                return Err(Write::MultipleLinks);
            }
            if file.group_blocks {
                return Err(Write::CannotKeepGroup);
            }
            if file.owner_blocks {
                return Err(Write::CannotKeepOwner);
            }
            if s.race.as_ref().is_some_and(|(raced, _)| *raced == k)
                && let Some((_, bytes)) = s.race.take()
                && let Some(file) = s.files.get_mut(&k)
            {
                file.bytes = bytes;
            }
            let now = s.files.get(&k).ok_or(Write::ChangedUnderfoot)?;
            if BytesHash::of(&now.bytes) != expect {
                return Err(Write::ChangedUnderfoot);
            }
            Ok(Guard)
        }
    }

    impl CorpusState {
        fn purge(&mut self, side: Visibility, text: TextHash) {
            self.purged.insert((side, text));
            self.objects.remove(&(side, text));
        }
    }

    /// The proof, inside the fake, that a write passed its comparisons.
    struct Guard;

    impl DocumentSource for FakeCorpus {
        type Error = FakeError;

        async fn list(&self) -> Result<DocumentListing, FakeError> {
            self.check(PortName::Documents)?;
            let s = self.lock();
            Ok(DocumentListing::new(
                s.documents.clone(),
                s.unreadable.clone(),
            ))
        }

        async fn bytes(&self, file: &FileFacts) -> Result<FileBytes, FakeError> {
            self.check(PortName::Documents)?;
            Ok(self
                .lock()
                .files
                .get(&key(file.location()))
                .map_or(FileBytes::Gone, |held| FileBytes::Read(held.bytes.clone())))
        }

        async fn private_root(&self) -> Result<PrivateRoot, FakeError> {
            self.check(PortName::Documents)?;
            Ok(self.lock().private_root)
        }
    }

    impl LedgerRead for FakeCorpus {
        type Error = FakeError;

        async fn read(&self, side: Visibility) -> Result<LedgerContents, FakeError> {
            self.check(PortName::Ledger)?;
            let s = self.lock();
            Ok(LedgerContents::new(
                s.ledgers.get(&side).cloned().unwrap_or_default(),
                s.corrupt.get(&side).cloned().unwrap_or_default(),
            ))
        }
    }

    impl LedgerAppend for FakeCorpus {
        type Error = FakeError;

        async fn append(
            &self,
            record: &crate::app::commit::record::Recordable,
        ) -> Result<(), FakeError> {
            self.check(PortName::Ledger)?;
            let mut s = self.lock();
            let judgement = record.judgement();
            let held = s.ledgers.entry(record.ledger()).or_default();
            if !held.iter().any(|r| {
                r.id() == judgement.id() && r.canonical_bytes() == judgement.canonical_bytes()
            }) {
                held.push(judgement.clone());
            }
            if let JudgementKind::Purge { object, .. } = judgement.kind() {
                s.purge(record.ledger(), *object);
            }
            Ok(())
        }
    }

    impl ObjectPut for FakeCorpus {
        type Error = FakeError;

        async fn put(&self, text: &Scanned<'_>) -> Result<Put, FakeError> {
            self.check(PortName::Objects)?;
            let mut s = self.lock();
            let hash = *text.key().text();
            if s.purged.contains(&(text.ledger(), hash)) {
                return Ok(Put::Purged);
            }
            s.objects
                .entry((text.ledger(), hash))
                .or_insert_with(|| text.text().to_owned());
            Ok(Put::Stored)
        }
    }

    impl ObjectRead for FakeCorpus {
        type Error = FakeError;

        async fn get(&self, side: Visibility, key: TextHash) -> Result<Option<String>, FakeError> {
            self.check(PortName::Objects)?;
            let s = self.lock();
            if s.purged.contains(&(side, key)) {
                return Ok(None);
            }
            Ok(s.objects.get(&(side, key)).cloned())
        }
    }

    impl ActorSource for FakeCorpus {
        type Error = FakeError;

        async fn actor(&self) -> Result<Judge, FakeError> {
            self.check(PortName::Actor)?;
            Ok(self.lock().actor.clone())
        }
    }

    impl KeySource for FakeCorpus {
        type Error = FakeError;

        async fn keys(&self) -> Result<Knowable<PinKeyring>, FakeError> {
            self.check(PortName::Keys)?;
            Ok(self.lock().keys.clone())
        }
    }

    impl ConfirmPort for FakeCorpus {
        type Error = FakeError;

        async fn ask(&self, prompt: &ConfirmPrompt) -> Result<Confirmed, FakeError> {
            self.check(PortName::Confirm)?;
            let mut s = self.lock();
            s.asked.push(*prompt);
            Ok(match s.reply {
                Reply::Always(answer) => answer,
                Reply::TypesBack(digest) if digest == prompt.digest() => Confirmed::Yes,
                Reply::TypesBack(_) => Confirmed::No,
            })
        }
    }

    impl FileWriter for FakeCorpus {
        type Error = FakeError;

        async fn create_new(&self, at: &Location, text: &[u8]) -> Result<Write, FakeError> {
            self.check(PortName::Files)?;
            let mut s = self.lock();
            if s.files.contains_key(&key(at)) {
                return Ok(Write::Exists);
            }
            s.files.insert(
                key(at),
                FakeFile {
                    bytes: text.to_vec(),
                    links: 1,
                    regular: true,
                    group_blocks: false,
                    owner_blocks: false,
                },
            );
            Ok(Write::Done)
        }

        async fn rewrite(
            &self,
            at: &Location,
            expect: BytesHash,
            text: &[u8],
        ) -> Result<Write, FakeError> {
            self.check(PortName::Files)?;
            match self.guarded(at, expect) {
                Ok(Guard) => {
                    if let Some(file) = self.lock().files.get_mut(&key(at)) {
                        file.bytes = text.to_vec();
                    }
                    Ok(Write::Done)
                }
                Err(refused) => Ok(refused),
            }
        }

        async fn remove(&self, at: &Location, expect: BytesHash) -> Result<Write, FakeError> {
            self.check(PortName::Files)?;
            match self.guarded(at, expect) {
                Ok(Guard) => {
                    self.lock().files.remove(&key(at));
                    Ok(Write::Done)
                }
                Err(refused) => Ok(refused),
            }
        }
    }

    impl RevisionFactsPort for FakeCorpus {
        type Error = FakeError;

        async fn baseline_facts(
            &self,
            _query: &BaselineQuery<'_>,
        ) -> Result<BaselineAnswer, FakeError> {
            self.check(PortName::RevisionFacts)?;
            Ok(self.lock().baseline)
        }

        async fn change_fingerprint(
            &self,
            _approved: &Revision,
        ) -> Result<Fingerprinting, FakeError> {
            self.check(PortName::RevisionFacts)?;
            Ok(self.lock().fingerprinting.clone())
        }

        async fn work_tree(&self) -> Result<Knowable<WorkTree>, FakeError> {
            self.check(PortName::RevisionFacts)?;
            Ok(self.lock().work_tree)
        }

        async fn author(&self, at: &Revision) -> Result<Knowable<Author>, FakeError> {
            self.check(PortName::RevisionFacts)?;
            Ok(self
                .lock()
                .authors
                .get(at)
                .cloned()
                .map_or(Knowable::Unknown, Knowable::Known))
        }

        async fn git_place(
            &self,
            root: Visibility,
            path: &RelPath,
        ) -> Result<Knowable<GitPlace>, FakeError> {
            self.check(PortName::RevisionFacts)?;
            Ok(self
                .lock()
                .places
                .get(&(root, path.clone()))
                .copied()
                .map_or(Knowable::Unknown, Knowable::Known))
        }
    }

    impl CorpusRead for FakeCorpus {
        type Documents = FakeCorpus;
        type Ledgers = FakeCorpus;
        type Objects = FakeCorpus;
        type Keys = FakeCorpus;
        type Facts = FakeCorpus;
        type Scanner = FakePorts;
        type Runner = FakePorts;

        fn documents(&self) -> &FakeCorpus {
            self
        }
        fn ledger(&self) -> &FakeCorpus {
            self
        }
        fn objects(&self) -> &FakeCorpus {
            self
        }
        fn key_source(&self) -> &FakeCorpus {
            self
        }
        fn facts(&self) -> &FakeCorpus {
            self
        }
        fn scanner(&self) -> &FakePorts {
            &self.shared
        }
        fn runner(&self) -> &FakePorts {
            &self.shared
        }
    }

    impl CorpusPorts for FakeCorpus {
        type Appender = FakeCorpus;
        type Texts = FakeCorpus;
        type Actor = FakeCorpus;
        type Confirm = FakeCorpus;
        type Files = FakeCorpus;
        type Time = FakePorts;
        type Ids = FakePorts;
        type Secrets = FakePorts;

        fn appender(&self) -> &FakeCorpus {
            self
        }
        fn put_texts(&self) -> &FakeCorpus {
            self
        }
        fn actor_source(&self) -> &FakeCorpus {
            self
        }
        fn confirm(&self) -> &FakeCorpus {
            self
        }
        fn files(&self) -> &FakeCorpus {
            self
        }
        fn time(&self) -> &FakePorts {
            &self.shared
        }
        fn ids(&self) -> &FakePorts {
            &self.shared
        }
        fn secrets(&self) -> &FakePorts {
            &self.shared
        }
    }
}

#[cfg(test)]
mod corpus_tests;
