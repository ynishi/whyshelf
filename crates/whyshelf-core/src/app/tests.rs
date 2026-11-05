//! The use cases, end to end, on the in-memory ports.
//!
//! Every test builds a `Workspace<FakePorts>`, drives it with `block_on`, and looks
//! at the result of the use case, at the events in the fake store, or at both.

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::approval::{Gate, TestOutcome};
use crate::domain::classification::{ConflictField, fold};
use crate::domain::event::{
    ClassChange, DocKind, EntityKind, Event, EventBody, Registration, StoreKind, Version,
};
use crate::domain::finding::FindingType;
use crate::domain::id::{Alias, DesignId, EntityId, FindingId, SpecId, test_support::eid};
use crate::domain::lint::{CheckViolation, UpstreamProblem};
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::query::Query;
use crate::domain::reference::{ItemRef, LocalRef, Ref};
use crate::domain::revision::{IntegrationFact, Revision, RevisionId};
use crate::domain::state::{
    Approval, BlockReason, Integration, InvalidReason, UnparsedReason, Upstream,
};
use crate::domain::tag::TagChange;

use super::Workspace;
use super::check::CmInput;
use super::config::{Config, IngestSettings};
use super::error::AppError;
use super::fake::{FakePorts, block_on, path, test_config};
use super::model::Model;
use super::ports::{ExportKind, StorePort};
use super::projection::MapDoc;
use super::query::{Field, GetResult, PageLimit, PageRequest, Target};
use super::register::AddOptions;
use super::view::{ExportProblem, NextAction, RowKind, StatusReport, StatusRow};

fn r(text: &str) -> Ref {
    text.parse().expect("a test reference")
}

fn it(text: &str) -> ItemRef {
    text.parse().expect("a test item reference")
}

fn a(text: &str) -> Alias {
    text.parse().expect("a test alias")
}

fn why(text: &str) -> NonBlank {
    NonBlank::new(text).expect("a test text")
}

/// A fake with a Finding, a Spec on it and a Design for it (one item that takes
/// the Spec's one pin), none of them registered yet.
fn files() -> FakePorts {
    let ports = FakePorts::new();
    ports.put_finding("docs/finding/slow.md", "Slow login", "p95 is 4s");
    ports.put_spec(
        "docs/spec/cache.md",
        &["finding/slow-login"],
        "cache the token",
        &["PIN-TTL"],
    );
    ports.put_design(
        "docs/design/cache.md",
        "spec/cache-timeout",
        &[(1, "add a ttl", &["PIN-TTL"])],
    );
    ports.set_pins(&["PIN-TTL"]);
    ports
}

fn register_chain(ws: &mut Workspace<FakePorts>) {
    let opts = |alias: &str| AddOptions::new().with_alias(a(alias));
    block_on(ws.add(&[path("docs/finding/slow.md")], &opts("finding/slow-login"))).expect("add");
    block_on(ws.add(&[path("docs/spec/cache.md")], &opts("spec/cache-timeout"))).expect("add");
    block_on(ws.add(&[path("docs/design/cache.md")], &opts("design/cache"))).expect("add");
}

fn workspace() -> Workspace<FakePorts> {
    let mut ws = Workspace::new(files(), test_config());
    register_chain(&mut ws);
    ws
}

/// Approve CR, CS, CI and CA in order.
fn approve_chain(ws: &mut Workspace<FakePorts>) {
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    block_on(ws.approve_cs(&r("spec/cache-timeout"), None)).expect("cs");
    block_on(ws.approve_ci(&r("design/cache"), None)).expect("ci");
    block_on(ws.approve_ca(&it("design/cache#1"), None, None)).expect("ca");
}

fn status(ws: &Workspace<FakePorts>) -> StatusReport {
    block_on(ws.status()).expect("status")
}

fn row<'a>(report: &'a StatusReport, alias: &str) -> &'a StatusRow {
    report
        .rows()
        .iter()
        .find(|r| r.alias().is_some_and(|x| x.as_str() == alias))
        .expect("a row with this alias")
}

fn item_row(report: &StatusReport) -> &StatusRow {
    report
        .rows()
        .iter()
        .find(|r| r.kind().entity() == Some(EntityKind::Item))
        .expect("an item row")
}

fn approval_of(row: &StatusRow) -> &Approval {
    row.state().expect("a state").approval()
}

fn upstream_of(row: &StatusRow) -> &Upstream {
    row.state().expect("a state").upstream()
}

fn id_of(ws: &Workspace<FakePorts>, alias: &str) -> EntityId {
    match block_on(ws.get(&Target::Entity(r(alias)))).expect("get") {
        GetResult::Entity(view) => view.id(),
        other => panic!("not an entity: {other:?}"),
    }
}

fn tips_of(ws: &Workspace<FakePorts>, alias: &str) -> usize {
    match block_on(ws.get(&Target::Entity(r(alias)))).expect("get") {
        GetResult::Entity(view) => view.tips().len(),
        other => panic!("not an entity: {other:?}"),
    }
}

fn add_tag(change: &str) -> NonEmptyVec<TagChange> {
    NonEmptyVec::new(change.parse().expect("a tag change"), Vec::new())
}

#[test]
fn register_ingest_check_and_status_end_to_end() {
    let mut ws = workspace();

    // Nothing changed since the registrations: ingest writes nothing.
    let events_before = ws.ports().events_in(StoreKind::Repo).len();
    let report = block_on(ws.ingest()).expect("ingest");
    assert!(report.revised().is_empty() && report.moved().is_empty());
    assert!(report.committed().is_none());
    assert_eq!(ws.ports().events_in(StoreKind::Repo).len(), events_before);

    // Before any approval everything is Draft, and the next step is a check.
    let s = status(&ws);
    assert_eq!(s.rows().len(), 4);
    assert!(matches!(
        approval_of(row(&s, "finding/slow-login")),
        Approval::Draft
    ));
    assert!(matches!(
        row(&s, "finding/slow-login").next(),
        NextAction::Approve { gate: Gate::Cr, .. }
    ));
    assert!(matches!(
        upstream_of(row(&s, "spec/cache-timeout")),
        Upstream::Blocked(_)
    ));

    // The machine layer reports without writing.
    let check = block_on(ws.check_cr(&r("finding/slow-login"))).expect("check");
    assert!(check.passed());
    assert_eq!(ws.ports().events_in(StoreKind::Repo).len(), events_before);

    approve_chain(&mut ws);

    let s = status(&ws);
    for alias in ["finding/slow-login", "spec/cache-timeout", "design/cache"] {
        assert!(
            matches!(approval_of(row(&s, alias)), Approval::Approved),
            "{alias}"
        );
        assert!(
            matches!(upstream_of(row(&s, alias)), Upstream::Ok),
            "{alias}"
        );
    }
    let item = item_row(&s);
    assert!(matches!(approval_of(item), Approval::Approved));
    assert!(matches!(
        item.integration(),
        Some(Integration::ApprovedPending { .. })
    ));
    assert_eq!(item.pins().map(|p| (p.found(), p.declared())), Some((1, 1)));
    assert!(matches!(item.next(), NextAction::AwaitIntegration));
    // The tests ran at the revision that was recorded.
    assert_eq!(ws.ports().tests_ran_at().len(), 1);

    // The approved revision reaches the baseline.
    ws.ports().integrate("rev-1");
    let s = status(&ws);
    assert!(matches!(
        item_row(&s).integration(),
        Some(Integration::Integrated { .. })
    ));
    assert!(matches!(
        row(&s, "spec/cache-timeout").next(),
        NextAction::AwaitingCm { .. }
    ));

    // The measurement is recorded; nothing is awaited any more.
    block_on(ws.record_cm(&r("spec/cache-timeout"), &CmInput::Pass, None)).expect("cm");
    let s = status(&ws);
    assert!(matches!(
        row(&s, "spec/cache-timeout").next(),
        NextAction::None
    ));
}

#[test]
fn revoke_then_approve_again_records_a_new_approval() {
    let mut ws = workspace();
    let first = block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    block_on(ws.revoke(first.event(), why("wrong source"), None)).expect("revoke");
    let s = status(&ws);
    assert!(matches!(
        approval_of(row(&s, "finding/slow-login")),
        Approval::Draft
    ));

    // Revoking twice is refused.
    assert!(matches!(
        block_on(ws.revoke(first.event(), why("again"), None)),
        Err(AppError::AlreadyRevoked { .. })
    ));

    let second = block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr again");
    assert_ne!(first.event(), second.event());
    let s = status(&ws);
    assert!(matches!(
        approval_of(row(&s, "finding/slow-login")),
        Approval::Approved
    ));
    // Both approvals are in the audit; the first is marked revoked.
    let audit = block_on(ws.audit(None)).expect("audit");
    let rows: Vec<_> = audit
        .rows()
        .iter()
        .filter(|r| r.gate() == Gate::Cr)
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().filter(|r| r.revoked()).count(), 1);
}

#[test]
fn editing_a_finding_makes_everything_below_it_stale() {
    let mut ws = workspace();
    approve_chain(&mut ws);

    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 9s");
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.revised().len(), 1);

    let s = status(&ws);
    assert!(matches!(
        approval_of(row(&s, "finding/slow-login")),
        Approval::Modified { .. }
    ));
    assert!(matches!(
        row(&s, "finding/slow-login").next(),
        NextAction::Reapprove { gate: Gate::Cr, .. }
    ));
    for alias in ["spec/cache-timeout", "design/cache"] {
        assert!(
            matches!(upstream_of(row(&s, alias)), Upstream::Stale(_)),
            "{alias}"
        );
    }
    assert!(matches!(upstream_of(item_row(&s)), Upstream::Stale(_)));
    assert!(matches!(
        row(&s, "spec/cache-timeout").next(),
        NextAction::Reapprove { gate: Gate::Cs, .. }
    ));
}

#[test]
fn a_supplementary_edit_revises_without_touching_approvals() {
    // Same parsed value, different text: the raw hash changes, the content hash
    // does not, and an approval still holds.
    let mut ws = workspace();
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    ws.ports()
        .put_finding_text_only("docs/finding/slow.md", "extra notes");
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.revised().len(), 1);
    let s = status(&ws);
    assert!(matches!(
        approval_of(row(&s, "finding/slow-login")),
        Approval::Approved
    ));
}

#[test]
fn a_dirty_worktree_refuses_ca_and_writes_nothing() {
    let mut ws = workspace();
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    block_on(ws.approve_cs(&r("spec/cache-timeout"), None)).expect("cs");
    block_on(ws.approve_ci(&r("design/cache"), None)).expect("ci");
    ws.ports().set_clean(false);
    let before = ws.ports().events_in(StoreKind::Repo).len();
    assert!(matches!(
        block_on(ws.approve_ca(&it("design/cache#1"), None, None)),
        Err(AppError::DirtyWorktree)
    ));
    assert_eq!(ws.ports().events_in(StoreKind::Repo).len(), before);
    // Tests did not even run.
    assert!(ws.ports().tests_ran_at().is_empty());
}

#[test]
fn ca_refuses_when_a_pin_is_missing_or_undeclared_or_tests_fail() {
    let mut ws = workspace();
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    block_on(ws.approve_cs(&r("spec/cache-timeout"), None)).expect("cs");
    block_on(ws.approve_ci(&r("design/cache"), None)).expect("ci");

    // The test carries a tag nobody declared and not the declared one.
    ws.ports().set_pins(&["UT-STRAY"]);
    ws.ports().set_tests(TestOutcome::Failed);
    let report = block_on(ws.check_ca(&it("design/cache#1"), None)).expect("check");
    assert!(!report.passed());
    assert!(
        report
            .violations()
            .iter()
            .any(|v| matches!(v, CheckViolation::PinMissingInTests { .. }))
    );
    assert!(
        report
            .violations()
            .iter()
            .any(|v| matches!(v, CheckViolation::PinUndeclaredInTests { .. }))
    );
    assert!(report.violations().contains(&CheckViolation::TestsFailed));

    let before = ws.ports().events_in(StoreKind::Repo).len();
    let refused = block_on(ws.approve_ca(&it("design/cache#1"), None, None));
    assert!(matches!(
        refused,
        Err(AppError::CheckFailed { gate: Gate::Ca, .. })
    ));
    assert_eq!(ws.ports().events_in(StoreKind::Repo).len(), before);
}

#[test]
fn ca_can_name_an_older_revision_and_records_that_one() {
    let mut ws = workspace();
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    block_on(ws.approve_cs(&r("spec/cache-timeout"), None)).expect("cs");
    block_on(ws.approve_ci(&r("design/cache"), None)).expect("ci");
    ws.ports().set_revision("rev-2");
    let older = super::raw::RawRevision::new("rev-1").expect("revision");
    let approved = block_on(ws.approve_ca(&it("design/cache#1"), Some(&older), None)).expect("ca");
    // What the approval hands back is what it recorded: the older revision, not the
    // one the working tree is at (a caller shows this, not the current one).
    let rev_1 = RevisionId::from_validated("rev-1".to_owned()).expect("a revision");
    assert_eq!(
        approved.attestation().revision(),
        Some(&Revision::Commit(rev_1))
    );
    // The tests ran at the revision that was recorded, not the current one.
    let ran: Vec<String> = ws
        .ports()
        .tests_ran_at()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(ran, ["rev-1"]);
    let bad = super::raw::RawRevision::new("nonsense").expect("text");
    assert!(matches!(
        block_on(ws.check_ca(&it("design/cache#1"), Some(&bad))),
        Err(AppError::RevisionRejected { .. })
    ));
}

#[test]
fn cm_needs_the_cs_approval_and_a_measured_finding_for_fail_hyp() {
    let mut ws = workspace();
    // No CS approval yet.
    let early = block_on(ws.record_cm(&r("spec/cache-timeout"), &CmInput::Pass, None));
    assert!(matches!(
        early,
        Err(AppError::CheckFailed { gate: Gate::Cm, ref violations })
            if violations.contains(&CheckViolation::CsApprovalNotFound)
    ));
    approve_chain(&mut ws);

    block_on(ws.record_cm(
        &r("spec/cache-timeout"),
        &CmInput::FailWiring {
            item_hint: why("the retry path"),
        },
        None,
    ))
    .expect("fail-wiring");

    // A usage observation is not a measurement.
    let wrong = block_on(ws.record_cm(
        &r("spec/cache-timeout"),
        &CmInput::FailHyp {
            finding: r("finding/slow-login"),
        },
        None,
    ));
    let finding = id_of(&ws, "finding/slow-login");
    assert!(matches!(
        wrong,
        Err(AppError::CheckFailed { gate: Gate::Cm, ref violations })
            if violations == &[CheckViolation::MeasurementNotMeasuredData {
                finding: FindingId::new(finding)
            }]
    ));

    ws.ports()
        .put_measured("docs/finding/measured.md", "Cache run", "rev-1");
    block_on(ws.add(
        &[path("docs/finding/measured.md")],
        &AddOptions::new().with_alias(a("finding/cache-run")),
    ))
    .expect("add");
    block_on(ws.record_cm(
        &r("spec/cache-timeout"),
        &CmInput::FailHyp {
            finding: r("finding/cache-run"),
        },
        None,
    ))
    .expect("fail-hyp");
}

#[test]
fn a_failing_subscriber_becomes_a_warning_and_the_write_stands() {
    let mut ws = workspace();
    ws.ports().fail_exports(true);
    let before = ws.ports().events_in(StoreKind::Repo).len();
    let committed =
        block_on(ws.tag(&r("finding/slow-login"), add_tag("+area:auth"))).expect("the write");
    assert_eq!(ws.ports().events_in(StoreKind::Repo).len(), before + 1);
    assert_eq!(committed.events().len(), 1);
    assert_eq!(committed.warnings().len(), 1);
    assert_eq!(committed.warnings()[0].name(), "map");
}

#[test]
fn subscribers_write_the_map_and_status_reports_a_hand_edit() {
    let mut ws = workspace();
    block_on(ws.tag(&r("finding/slow-login"), add_tag("+area:auth"))).expect("tag");
    assert!(ws.ports().export_writes() > 0);
    assert!(status(&ws).exports().is_empty());

    ws.ports()
        .edit_export(StoreKind::Repo, ExportKind::Map, "hand edited");
    let s = status(&ws);
    assert_eq!(s.exports().len(), 1);
    assert_eq!(s.exports()[0].problem, ExportProblem::EditedByHand);

    // A rebuild writes it again, even though no event was needed.
    let report = block_on(ws.rebuild()).expect("rebuild");
    assert!(report.warnings().is_empty());
    assert!(status(&ws).exports().is_empty());
}

#[test]
fn two_writes_in_a_row_see_one_tip_and_a_fork_heals_on_the_next_write() {
    let mut ws = workspace();
    block_on(ws.tag(&r("finding/slow-login"), add_tag("+area:auth"))).expect("tag");
    block_on(ws.tag(&r("finding/slow-login"), add_tag("+wip"))).expect("tag");
    assert_eq!(tips_of(&ws, "finding/slow-login"), 1);
    let events = ws.ports().events_in(StoreKind::Repo);
    let id = id_of(&ws, "finding/slow-login");
    let stream: Vec<&Event> = events.iter().filter(|e| e.target() == id).collect();
    let last = stream.last().expect("an event");
    let before_last = stream[stream.len() - 2];
    assert_eq!(last.heads(), &BTreeSet::from([before_last.id()]));

    // Two branches each add an event from the same tips, then merge.
    let tips = {
        let model = block_on(Model::load(ws.ports(), ws.config())).expect("model");
        model.tips_in(StoreKind::Repo, id)
    };
    for tag in ["x1", "x2"] {
        let change = NonEmptyVec::new(
            ClassChange::Tag(add_tag(&format!("+{tag}")).first().clone()),
            Vec::new(),
        );
        let event = Event::new(
            id,
            tips.clone(),
            events[0].at().clone(),
            EventBody::Reclassified { changes: change },
        )
        .expect("event");
        block_on(StorePort::append(ws.ports(), StoreKind::Repo, &event)).expect("append");
    }
    assert_eq!(tips_of(&ws, "finding/slow-login"), 2);
    // The next write lists both, so the stream is one again and no tag is lost.
    block_on(ws.tag(&r("finding/slow-login"), add_tag("+healed"))).expect("tag");
    assert_eq!(tips_of(&ws, "finding/slow-login"), 1);
    let GetResult::Entity(view) =
        block_on(ws.get(&Target::Entity(r("finding/slow-login")))).expect("get")
    else {
        panic!("an entity");
    };
    assert_eq!(view.tags().len(), 5);
}

#[test]
fn tags_must_be_in_the_vocabulary_and_aliases_free() {
    let mut ws = workspace();
    assert!(matches!(
        block_on(ws.tag(&r("finding/slow-login"), add_tag("+area:nothing"))),
        Err(AppError::Vocabulary(_))
    ));
    let taken = NonEmptyVec::new(ClassChange::SetAlias(a("spec/cache-timeout")), Vec::new());
    assert!(matches!(
        block_on(ws.reclassify(&r("finding/slow-login"), taken)),
        Err(AppError::AliasTaken { .. })
    ));
    // Tagging again with a vocabulary tag works, and removal too.
    block_on(ws.tag(&r("finding/slow-login"), add_tag("+area:auth"))).expect("tag");
    block_on(ws.tag(&r("finding/slow-login"), add_tag("-area:auth"))).expect("untag");
    let GetResult::Entity(view) =
        block_on(ws.get(&Target::Entity(r("finding/slow-login")))).expect("get")
    else {
        panic!("an entity");
    };
    assert!(view.tags().is_empty());
}

#[test]
fn add_checks_kind_and_a_secret_stops_the_write() {
    let mut ws = Workspace::new(files(), test_config());
    let wrong = block_on(ws.add(
        &[path("docs/spec/cache.md")],
        &AddOptions::new().with_kind(crate::domain::event::DocKind::Finding),
    ));
    assert!(matches!(wrong, Err(AppError::KindMismatch { .. })));

    ws.ports()
        .put_finding("docs/finding/key.md", "Key", "token AKIA-SECRET");
    let leaked = block_on(ws.add(&[path("docs/finding/key.md")], &AddOptions::new()));
    assert!(matches!(leaked, Err(AppError::SecretFound { .. })));
    assert!(ws.ports().events_in(StoreKind::Repo).is_empty());
    assert_eq!(ws.ports().object_count(StoreKind::Repo), 0);

    // Adding a directory registers what is not registered and skips the rest.
    ws.ports().remove_file("docs/finding/key.md");
    let added = block_on(ws.add(&[path("docs/finding")], &AddOptions::new())).expect("add");
    assert_eq!(added.registered().len(), 1);
    let again = block_on(ws.add(&[path("docs/finding")], &AddOptions::new())).expect("add");
    assert!(again.registered().is_empty());
    assert_eq!(again.skipped().len(), 1);
}

#[test]
fn unregistered_files_are_listed_never_taken_in() {
    let mut ws = workspace();
    ws.ports()
        .put_finding("docs/notes/idea.md", "Idea", "maybe");
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.unregistered(), [path("docs/notes/idea.md")]);
    let s = status(&ws);
    assert_eq!(s.unregistered(), [path("docs/notes/idea.md")]);
    assert_eq!(
        StatusReport::next_for_unregistered(&s.unregistered()[0]).to_string(),
        "whyshelf add docs/notes/idea.md"
    );
}

#[test]
fn a_renamed_file_is_a_move_by_its_text() {
    let mut ws = workspace();
    let id = id_of(&ws, "finding/slow-login");
    ws.ports()
        .rename_file("docs/finding/slow.md", "docs/finding/slow-login.md");
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.moved().len(), 1);
    assert_eq!(report.moved()[0].0, id);
    assert!(report.missing().is_empty());
    assert!(report.unregistered().is_empty());
    let s = status(&ws);
    assert_eq!(
        row(&s, "finding/slow-login").path().map(|p| p.as_str()),
        Some("docs/finding/slow-login.md")
    );
    // No Revised: only the Moved.
    let events = ws.ports().events_in(StoreKind::Repo);
    assert!(
        events
            .iter()
            .any(|e| matches!(e.body(), EventBody::Moved { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e.body(), EventBody::Revised { .. }))
    );
}

#[test]
fn a_deleted_file_writes_no_event_and_is_reported_missing() {
    let mut ws = workspace();
    approve_chain(&mut ws);
    let before = ws.ports().events_in(StoreKind::Repo).len();
    ws.ports().remove_file("docs/finding/slow.md");
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.missing(), [path("docs/finding/slow.md")]);
    assert_eq!(ws.ports().events_in(StoreKind::Repo).len(), before);
    let s = status(&ws);
    assert_eq!(s.missing(), [path("docs/finding/slow.md")]);
    assert!(matches!(
        row(&s, "finding/slow-login").next(),
        NextAction::RestoreFile
    ));
    // The Spec on it now has a missing upstream: Invalid.
    assert!(matches!(
        approval_of(row(&s, "spec/cache-timeout")),
        Approval::Invalid(_)
    ));
}

#[test]
fn a_file_that_stops_parsing_is_recorded_as_a_version_and_is_invalid() {
    let mut ws = workspace();
    ws.ports()
        .put_broken("docs/finding/slow.md", "<<<<<<< ours\np95 is 4s\n=======\n");
    let objects_before = ws.ports().object_count(StoreKind::Repo);
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.revised().len(), 1);
    assert_eq!(ws.ports().object_count(StoreKind::Repo), objects_before + 1);
    let events = ws.ports().events_in(StoreKind::Repo);
    let last = events.last().expect("an event");
    assert!(matches!(
        last.body(),
        EventBody::Revised {
            new: Version::Unparsed { .. },
            ..
        }
    ));
    let s = status(&ws);
    assert!(matches!(
        approval_of(row(&s, "finding/slow-login")),
        Approval::Invalid(_)
    ));
    assert!(matches!(
        row(&s, "finding/slow-login").next(),
        NextAction::FixFile
    ));

    // Fixing the file is the next Revised.
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 4s");
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.revised().len(), 1);
    assert!(matches!(
        approval_of(row(&status(&ws), "finding/slow-login")),
        Approval::Draft
    ));
}

#[test]
fn a_new_item_number_in_a_design_is_registered_with_a_new_identifier() {
    let mut ws = workspace();
    let item = |s: &StatusReport| {
        s.rows()
            .iter()
            .filter(|r| r.kind().entity() == Some(EntityKind::Item))
            .count()
    };
    assert_eq!(item(&status(&ws)), 1);
    ws.ports().put_design(
        "docs/design/cache.md",
        "spec/cache-timeout",
        &[(1, "add a ttl", &["PIN-TTL"]), (2, "log it", &[])],
    );
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.items_registered(), 1);
    assert_eq!(item(&status(&ws)), 2);
    // Ingest again: nothing new.
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.items_registered(), 0);
    // An item whose number leaves the Design is no longer shown, its events stay.
    ws.ports().put_design(
        "docs/design/cache.md",
        "spec/cache-timeout",
        &[(1, "add a ttl", &["PIN-TTL"])],
    );
    block_on(ws.ingest()).expect("ingest");
    assert_eq!(item(&status(&ws)), 1);
    let registered = ws
        .ports()
        .events_in(StoreKind::Repo)
        .iter()
        .filter(|e| matches!(e.body(), EventBody::Registered(Registration::Item { .. })))
        .count();
    assert_eq!(registered, 2);
}

#[test]
fn many_changed_files_rebuild_the_cache_instead_of_diffing() {
    let config: Config = test_config().with_ingest(IngestSettings::new(1));
    let mut ws = Workspace::new(files(), config);
    register_chain(&mut ws);
    block_on(ws.ingest()).expect("ingest");
    assert_eq!(ws.ports().cache_rebuilds(), 0);
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 5s");
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &["finding/slow-login"],
        "cache the token longer",
        &["PIN-TTL"],
    );
    let report = block_on(ws.ingest()).expect("ingest");
    assert!(report.rebuilt());
    assert_eq!(ws.ports().cache_rebuilds(), 1);
    assert_eq!(report.revised().len(), 2);
    assert_eq!(ws.ports().cache_len(), 3);
}

#[test]
fn ingest_blocks_a_file_with_a_secret_and_still_records_the_others() {
    let mut ws = workspace();
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "key AKIA-SECRET");
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &["finding/slow-login"],
        "cache the token longer",
        &["PIN-TTL"],
    );
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.blocked().len(), 1);
    assert_eq!(report.blocked()[0].path(), &path("docs/finding/slow.md"));
    assert!(matches!(
        report.blocked()[0].error(),
        AppError::SecretFound { .. }
    ));
    assert_eq!(report.revised().len(), 1);
    // The blocked file is tried again next time.
    let report = block_on(ws.ingest()).expect("ingest");
    assert_eq!(report.blocked().len(), 1);
}

#[test]
fn get_resolves_alias_id_private_ext_url_and_item_references() {
    let mut ws = workspace();
    let id = id_of(&ws, "spec/cache-timeout");

    // By identifier, same entity.
    let by_id = block_on(ws.get(&id.to_string().parse().expect("target"))).expect("get");
    assert!(matches!(by_id, GetResult::Entity(v) if v.id() == id));

    // A private or external reference that cannot be looked up is shown, not an error.
    let private = format!("private:{}", eid(999));
    assert!(matches!(
        block_on(ws.get(&private.parse().expect("target"))).expect("get"),
        GetResult::Unresolved(Ref::Private(_))
    ));
    assert!(matches!(
        block_on(ws.get(&"ext:other-project/spec/auth".parse().expect("target"))).expect("get"),
        GetResult::Unresolved(Ref::External { .. })
    ));
    assert!(matches!(
        block_on(ws.get(&"https://example.org/doc#part".parse().expect("target"))).expect("get"),
        GetResult::External(_)
    ));

    // Nothing by that name.
    assert!(matches!(
        block_on(ws.get(&"spec/nothing".parse().expect("target"))),
        Err(AppError::NotFound { .. })
    ));

    // An item by its Design, and by its Spec while only one Design refers to it.
    for text in ["design/cache#1", "spec/cache-timeout#1"] {
        let got = block_on(ws.get(&text.parse().expect("target"))).expect("get");
        assert!(
            matches!(got, GetResult::Entity(v) if v.kind() == EntityKind::Item),
            "{text}"
        );
    }
    // A second Design for the Spec makes `<spec>#1` ambiguous.
    ws.ports().put_design(
        "docs/design/other.md",
        "spec/cache-timeout",
        &[(1, "x", &[])],
    );
    block_on(ws.add(
        &[path("docs/design/other.md")],
        &AddOptions::new().with_alias(a("design/other")),
    ))
    .expect("add");
    assert!(matches!(
        block_on(ws.get(&"spec/cache-timeout#1".parse().expect("target"))),
        Err(AppError::ItemRef(
            crate::domain::error::ItemRefError::Ambiguous { count: 2 }
        ))
    ));
    assert!(matches!(
        block_on(ws.get(&"design/cache#1".parse().expect("target"))).expect("get"),
        GetResult::Entity(_)
    ));
    // A number that is not registered.
    assert!(matches!(
        block_on(ws.get(&"design/cache#7".parse().expect("target"))),
        Err(AppError::ItemNotFound { .. })
    ));
}

#[test]
fn find_filters_by_classification_state_relation_and_text_and_pages() {
    let mut ws = workspace();
    block_on(ws.tag(&r("finding/slow-login"), add_tag("+area:auth"))).expect("tag");
    ws.ports()
        .put_finding("docs/finding/lonely.md", "Lonely", "nobody cites me");
    block_on(ws.add(
        &[path("docs/finding/lonely.md")],
        &AddOptions::new().with_alias(a("finding/lonely")),
    ))
    .expect("add");

    let find = |text: &str| {
        let query = Query::parse(text).expect("query");
        block_on(ws.find(&query, &PageRequest::default())).expect("find")
    };
    assert_eq!(find("tag:area:auth").rows().len(), 1);
    assert_eq!(find("kind:spec state:draft").rows().len(), 1);
    assert_eq!(find("state:approved").rows().len(), 0);
    assert_eq!(find("").total(), 5);
    let grounded = find("grounds:finding/slow-login");
    assert_eq!(grounded.rows().len(), 1);
    assert_eq!(grounded.rows()[0].kind, EntityKind::Spec);
    let upstream = find("upstream:design/cache");
    assert_eq!(upstream.rows()[0].kind, EntityKind::Item);
    let orphans = find("orphan:true");
    assert_eq!(orphans.rows().len(), 1);
    assert_eq!(
        orphans.rows()[0].alias.as_ref().map(Alias::as_str),
        Some("finding/lonely")
    );
    // Full text goes through the cache; an item is searched by its summary.
    assert_eq!(find("nobody").rows().len(), 1);
    // The summary is in the Design's text and in the item itself.
    assert_eq!(find("\"add a ttl\"").rows().len(), 2);
    assert!(matches!(
        block_on(ws.find(
            &Query::parse("grounds:finding/none").expect("q"),
            &PageRequest::default()
        )),
        Err(AppError::NotFound { .. })
    ));

    // Pages: two at a time, continued by the cursor, with chosen fields.
    let query = Query::parse("").expect("query");
    let mut request = PageRequest {
        limit: PageLimit::new(2),
        cursor: None,
        fields: BTreeSet::from([Field::Alias]),
    };
    let mut seen = Vec::new();
    loop {
        let page = block_on(ws.find(&query, &request)).expect("find");
        assert!(page.rows().len() <= 2);
        assert!(
            page.rows()
                .iter()
                .all(|r| r.path.is_none() && r.tags.is_none())
        );
        seen.extend(page.rows().iter().map(|r| r.id));
        match page.next() {
            Some(cursor) => request.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(seen.len(), 5);
    assert_eq!(seen.iter().collect::<BTreeSet<_>>().len(), 5);
    assert_eq!(PageLimit::new(0).get(), 1);
    assert_eq!(PageLimit::new(10_000).get(), PageLimit::MAX);
}

#[test]
fn a_text_search_that_matches_more_files_than_one_port_call_gives_still_counts_them_all() {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    let paths: Vec<_> = (0..1100)
        .map(|n| {
            let at = format!("docs/finding/f{n}.md");
            ws.ports()
                .put_finding(&at, &format!("Finding {n}"), &format!("common words {n}"));
            path(&at)
        })
        .collect();
    block_on(ws.add(&paths, &AddOptions::new())).expect("add");
    let page = block_on(ws.find(
        &Query::parse("common").expect("query"),
        &PageRequest::default(),
    ))
    .expect("find");
    assert_eq!(page.total(), 1100);
}

#[test]
fn publish_checks_the_leak_lint_and_the_repo_store_folds_alone_to_the_same_classification() {
    let config = test_config().with_default_store(StoreKind::Private);
    let mut ws = Workspace::new(files(), config);
    register_chain(&mut ws);
    block_on(ws.tag(&r("spec/cache-timeout"), add_tag("+area:auth"))).expect("tag");
    assert!(ws.ports().events_in(StoreKind::Repo).is_empty());

    // The Spec's text names a private Finding by alias: refused, nothing written.
    let refused = block_on(ws.publish(&r("spec/cache-timeout")));
    assert!(matches!(refused, Err(AppError::LeakFound { .. })));
    assert!(ws.ports().events_in(StoreKind::Repo).is_empty());

    // Publish the Finding first, then the Spec has no private name left to carry.
    block_on(ws.publish(&r("finding/slow-login"))).expect("publish finding");
    block_on(ws.publish(&r("spec/cache-timeout"))).expect("publish spec");
    assert!(matches!(
        block_on(ws.publish(&r("spec/cache-timeout"))),
        Err(AppError::AlreadyPublished { .. })
    ));

    let id = id_of(&ws, "spec/cache-timeout");
    let repo = fold(&ws.ports().events_in(StoreKind::Repo));
    let private = fold(&ws.ports().events_in(StoreKind::Private));
    let (from_repo, from_private) = (
        repo.doc(id).expect("repo"),
        private.doc(id).expect("private"),
    );
    assert_eq!(from_repo.tags(), from_private.tags());
    assert_eq!(from_repo.alias(), from_private.alias());
    assert_eq!(from_repo.path(), from_private.path());
    assert_eq!(from_repo.kind(), from_private.kind());
    // The latest text is in the repo store; the earlier history stays private.
    assert!(ws.ports().object_count(StoreKind::Repo) >= 2);

    // The entity now lives in the repo store, and a write goes there.
    block_on(ws.tag(&r("spec/cache-timeout"), add_tag("+wip"))).expect("tag");
    assert_eq!(tips_of(&ws, "spec/cache-timeout"), 1);
    let GetResult::Entity(view) =
        block_on(ws.get(&Target::Entity(r("spec/cache-timeout")))).expect("get")
    else {
        panic!("an entity");
    };
    assert_eq!(view.store(), StoreKind::Repo);
}

#[test]
fn publish_refuses_an_alias_the_repo_store_already_holds() {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    ws.ports().put_finding("docs/finding/a.md", "First", "one");
    ws.ports().put_finding("docs/finding/b.md", "Second", "two");
    add_to(
        &mut ws,
        "docs/finding/a.md",
        "finding/x",
        StoreKind::Private,
    );
    add_to(&mut ws, "docs/finding/b.md", "finding/x", StoreKind::Repo);
    let registered = |store: StoreKind| {
        ws.ports()
            .events_in(store)
            .iter()
            .find_map(|e| {
                matches!(e.body(), EventBody::Registered(Registration::Doc(_))).then(|| e.target())
            })
            .expect("a registration")
    };
    let (held, mine) = (registered(StoreKind::Repo), registered(StoreKind::Private));

    let refused = block_on(ws.publish(&Ref::Private(mine)));
    assert!(
        matches!(refused, Err(AppError::AliasTaken { holder, .. }) if holder == held),
        "{refused:?}"
    );
    assert_eq!(ws.ports().events_in(StoreKind::Repo).len(), 1);
}

#[test]
fn the_repo_map_shows_a_private_ground_only_as_an_opaque_reference() {
    let config = test_config().with_default_store(StoreKind::Private);
    let mut ws = Workspace::new(FakePorts::new(), config);
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 4s");
    block_on(ws.add(
        &[path("docs/finding/slow.md")],
        &AddOptions::new().with_alias(a("finding/slow-login")),
    ))
    .expect("add");
    let finding = id_of(&ws, "finding/slow-login");
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &[&format!("private:{finding}")],
        "cache the token",
        &["PIN-TTL"],
    );
    block_on(
        ws.add(
            &[path("docs/spec/cache.md")],
            &AddOptions::new()
                .with_alias(a("spec/cache-timeout"))
                .with_store(StoreKind::Repo),
        ),
    )
    .expect("add");
    let spec = id_of(&ws, "spec/cache-timeout");

    let model = block_on(Model::load(ws.ports(), ws.config())).expect("model");
    let map = MapDoc::build(&model, StoreKind::Repo);
    assert!(!map.entities().contains_key(&finding));
    let entity = &map.entities()[&spec];
    assert!(entity.refs().grounds().is_empty());
    assert_eq!(
        entity.refs().external(),
        &BTreeSet::from([Ref::Private(finding)])
    );
    // The private map has the Finding, with its alias and title.
    let private = MapDoc::build(&model, StoreKind::Private);
    let f = &private.entities()[&finding];
    assert_eq!(f.alias().map(Alias::as_str), Some("finding/slow-login"));
    assert_eq!(f.title(), Some("Slow login"));
    // Deterministic: the same model, the same map.
    assert_eq!(map, MapDoc::build(&model, StoreKind::Repo));
}

#[test]
fn scaffolds_create_and_register_with_an_alias() {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    let finding = block_on(ws.finding_new(FindingType::UsageObservation, why("Slow login"), None))
        .expect("finding new");
    assert_eq!(finding.alias().as_str(), "finding/slow-login");
    assert_eq!(finding.path().as_str(), "docs/finding/slow-login.md");
    assert!(ws.ports().has_file("docs/finding/slow-login.md"));

    let spec =
        block_on(ws.spec_new(a("spec/cache-timeout"), &r("finding/slow-login"))).expect("spec new");
    assert_eq!(spec.path().as_str(), "docs/spec/cache-timeout.md");
    let design =
        block_on(ws.design_new(a("design/cache"), &r("spec/cache-timeout"))).expect("design new");
    assert_eq!(design.path().as_str(), "docs/design/cache.md");
    let s = status(&ws);
    assert_eq!(s.rows().len(), 3);

    // The alias is taken; an existing file is never overwritten.
    assert!(matches!(
        block_on(ws.finding_new(
            FindingType::UsageObservation,
            why("Other"),
            Some(a("finding/slow-login"))
        )),
        Err(AppError::AliasTaken { .. })
    ));
    ws.ports().put_finding("docs/finding/other.md", "x", "y");
    assert!(matches!(
        block_on(ws.finding_new(FindingType::UsageObservation, why("Other"), None)),
        Err(AppError::PathExists { .. })
    ));
    assert!(matches!(
        block_on(ws.finding_new(FindingType::UsageObservation, why("!!!"), None)),
        Err(AppError::NoSlug)
    ));
}

#[test]
fn a_scaffold_that_the_gate_refuses_leaves_no_file_and_can_be_run_again() {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    ws.ports()
        .put_finding("docs/finding/secret.md", "Secret plan", "x");
    add_to(
        &mut ws,
        "docs/finding/secret.md",
        "finding/secret",
        StoreKind::Private,
    );

    // The public file would carry the title of a private Finding.
    let refused = block_on(ws.finding_new(
        FindingType::UsageObservation,
        why("Secret plan B"),
        Some(a("finding/b")),
    ));
    assert!(
        matches!(refused, Err(AppError::LeakFound { .. })),
        "{refused:?}"
    );
    assert!(
        !ws.ports().has_file("docs/finding/b.md"),
        "the file this call made is taken away again"
    );

    let again = block_on(ws.finding_new(
        FindingType::UsageObservation,
        why("Quiet plan B"),
        Some(a("finding/b")),
    ));
    assert!(again.is_ok(), "{again:?}");
    assert!(ws.ports().has_file("docs/finding/b.md"));
}

#[test]
fn mv_records_a_move_and_refuses_an_occupied_or_missing_path() {
    let mut ws = workspace();
    assert!(matches!(
        block_on(ws.mv(&r("finding/slow-login"), path("docs/finding/new.md"))),
        Err(AppError::FileMissing { .. })
    ));
    ws.ports()
        .put_finding("docs/finding/new.md", "Slow login", "p95 is 4s");
    assert!(matches!(
        block_on(ws.mv(&r("finding/slow-login"), path("docs/spec/cache.md"))),
        Err(AppError::PathOccupied { .. })
    ));
    block_on(ws.mv(&r("finding/slow-login"), path("docs/finding/new.md"))).expect("mv");
    let s = status(&ws);
    assert_eq!(
        row(&s, "finding/slow-login").path().map(|p| p.as_str()),
        Some("docs/finding/new.md")
    );
}

#[test]
fn purge_deletes_the_text_but_not_the_history() {
    let mut ws = workspace();
    let events = ws.ports().events_in(StoreKind::Repo);
    let raw = events
        .iter()
        .find_map(|e| match e.body() {
            EventBody::Registered(Registration::Doc(d))
                if d.path().as_str().ends_with("slow.md") =>
            {
                Some(*d.first().raw())
            }
            _ => None,
        })
        .expect("the registration");
    let objects = ws.ports().object_count(StoreKind::Repo);
    block_on(ws.purge(&r("finding/slow-login"), raw, why("secret slipped"))).expect("purge");
    assert_eq!(ws.ports().object_count(StoreKind::Repo), objects - 1);
    assert!(
        ws.ports()
            .events_in(StoreKind::Repo)
            .iter()
            .any(|e| matches!(e.body(), EventBody::Purged { .. }))
    );
    let other = crate::domain::hash::ContentHash::raw(b"nothing");
    assert!(matches!(
        block_on(ws.purge(&r("finding/slow-login"), other, why("x"))),
        Err(AppError::NotAVersion { .. })
    ));
}

#[test]
fn purging_a_text_two_documents_share_records_it_for_both_and_neither_reads_as_missing() {
    let config = test_config().with_default_store(StoreKind::Private);
    let mut ws = Workspace::new(FakePorts::new(), config);
    // Two documents with the very same text are stored once, under one raw hash.
    ws.ports().put_finding("docs/finding/a.md", "Same", "words");
    ws.ports().put_finding("docs/finding/b.md", "Same", "words");
    add_to(
        &mut ws,
        "docs/finding/a.md",
        "finding/a",
        StoreKind::Private,
    );
    add_to(
        &mut ws,
        "docs/finding/b.md",
        "finding/b",
        StoreKind::Private,
    );
    let raw = ws
        .ports()
        .events_in(StoreKind::Private)
        .iter()
        .find_map(|e| match e.body() {
            EventBody::Registered(Registration::Doc(d)) => Some(*d.first().raw()),
            _ => None,
        })
        .expect("a registration");
    assert_eq!(ws.ports().object_count(StoreKind::Private), 1);

    block_on(ws.purge(&r("finding/a"), raw, why("secret slipped"))).expect("purge");

    let purged: BTreeSet<EntityId> = ws
        .ports()
        .events_in(StoreKind::Private)
        .iter()
        .filter(|e| matches!(e.body(), EventBody::Purged { .. }))
        .map(|e| e.target())
        .collect();
    assert_eq!(
        purged,
        BTreeSet::from([id_of(&ws, "finding/a"), id_of(&ws, "finding/b")]),
        "the text left the store for both, and both say so"
    );
    assert_eq!(ws.ports().object_count(StoreKind::Private), 0);
    // The other document's read of that text says it was purged.
    assert!(matches!(
        block_on(ws.publish(&r("finding/b"))),
        Err(AppError::ObjectPurged { .. })
    ));
}

#[test]
fn views_trace_changelog_audit_and_context() {
    let mut ws = workspace();
    approve_chain(&mut ws);

    let trace = block_on(ws.trace(&r("spec/cache-timeout"))).expect("trace");
    let kinds: Vec<EntityKind> = trace.nodes().iter().map(|n| n.kind()).collect();
    assert_eq!(
        kinds,
        [
            EntityKind::Finding,
            EntityKind::Spec,
            EntityKind::Design,
            EntityKind::Item
        ]
    );
    assert_eq!(trace.links().len(), 3);
    let item = trace.nodes().last().expect("an item");
    assert_eq!(item.approved().len(), 1);
    assert_eq!(
        item.approved()[0].integrated,
        IntegrationFact::NotIntegrated
    );

    let log = block_on(ws.changelog(&r("finding/slow-login"))).expect("changelog");
    assert!(matches!(
        log.entries()[0].event().body(),
        EventBody::Registered(_)
    ));
    assert!(
        log.entries()
            .iter()
            .any(|e| matches!(e.event().body(), EventBody::Approved { .. }))
    );

    let audit = block_on(ws.audit(None)).expect("audit");
    assert_eq!(audit.rows().len(), 4);
    let ca = audit
        .rows()
        .iter()
        .find(|r| r.gate() == Gate::Ca)
        .expect("ca");
    assert!(ca.revision().is_some() && ca.tests() == Some(TestOutcome::Passed));
    assert_eq!(ca.by(), &Actor::new("alice").expect("name"));
    let only_specs =
        block_on(ws.audit(Some(&Query::parse("kind:spec").expect("q")))).expect("audit");
    assert_eq!(only_specs.rows().len(), 1);

    let context = block_on(ws.context()).expect("context");
    assert_eq!(
        context.line().to_string(),
        "[context: project=demo, worktree=/work/demo, revision=rev-1 (main)]"
    );
    assert!(context.events_in(StoreKind::Repo) > 0);
    assert_eq!(context.events_in(StoreKind::Private), 0);
}

#[test]
fn an_approval_needs_a_name() {
    let config = {
        let base = test_config();
        // A configuration without a default approver.
        Config::new(
            base.project().clone(),
            base.paths().clone(),
            base.test().clone(),
        )
        .with_default_store(StoreKind::Repo)
    };
    let mut ws = Workspace::new(files(), config);
    register_chain(&mut ws);
    assert!(matches!(
        block_on(ws.approve_cr(&r("finding/slow-login"), None)),
        Err(AppError::NoActor)
    ));
    let bob = Actor::new("bob").expect("name");
    block_on(ws.approve_cr(&r("finding/slow-login"), Some(bob))).expect("cr");
}

// --- references into a store that cannot be read ----------------------------
//
// A "clone" below is the same files and the repo store without the private store
// (`drop_private_store`): what a CI machine or a public checkout has.

fn add_to(ws: &mut Workspace<FakePorts>, at: &str, alias: &str, store: StoreKind) {
    let options = AddOptions::new().with_alias(a(alias)).with_store(store);
    block_on(ws.add(&[path(at)], &options)).expect("add");
}

/// A private Finding, a public Spec that names it as `private:<id>`, and a public
/// Design on the Spec.
fn private_ground_world() -> (Workspace<FakePorts>, EntityId) {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 4s");
    add_to(
        &mut ws,
        "docs/finding/slow.md",
        "finding/slow-login",
        StoreKind::Private,
    );
    let finding = id_of(&ws, "finding/slow-login");
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &[&format!("private:{finding}")],
        "cache the token",
        &["PIN-TTL"],
    );
    ws.ports().put_design(
        "docs/design/cache.md",
        "spec/cache-timeout",
        &[(1, "add a ttl", &["PIN-TTL"])],
    );
    ws.ports().set_pins(&["PIN-TTL"]);
    add_to(
        &mut ws,
        "docs/spec/cache.md",
        "spec/cache-timeout",
        StoreKind::Repo,
    );
    add_to(
        &mut ws,
        "docs/design/cache.md",
        "design/cache",
        StoreKind::Repo,
    );
    (ws, finding)
}

/// A public Finding and a private one, both grounds of a public Spec (the private
/// one as `private:<id>`), and a public Design on the Spec. Returns the private
/// Finding.
fn two_ground_world() -> (Workspace<FakePorts>, EntityId) {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 4s");
    ws.ports()
        .put_finding("docs/finding/cold.md", "Cold start", "9s");
    add_to(
        &mut ws,
        "docs/finding/slow.md",
        "finding/slow-login",
        StoreKind::Repo,
    );
    add_to(
        &mut ws,
        "docs/finding/cold.md",
        "finding/cold-start",
        StoreKind::Private,
    );
    let cold = id_of(&ws, "finding/cold-start");
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &["finding/slow-login", &format!("private:{cold}")],
        "cache the token",
        &["PIN-TTL"],
    );
    ws.ports().put_design(
        "docs/design/cache.md",
        "spec/cache-timeout",
        &[(1, "add a ttl", &["PIN-TTL"])],
    );
    ws.ports().set_pins(&["PIN-TTL"]);
    add_to(
        &mut ws,
        "docs/spec/cache.md",
        "spec/cache-timeout",
        StoreKind::Repo,
    );
    add_to(
        &mut ws,
        "docs/design/cache.md",
        "design/cache",
        StoreKind::Repo,
    );
    (ws, cold)
}

/// What a row cannot read here, as `(root, upstream)`.
fn unreadable(row: &StatusRow) -> Vec<(EntityId, EntityId)> {
    row.state()
        .expect("a state")
        .completeness()
        .reasons()
        .map(|reason| (reason.root(), reason.upstream()))
        .collect()
}

fn upstream_violation(upstream: EntityId, problem: UpstreamProblem) -> CheckViolation {
    CheckViolation::Upstream { upstream, problem }
}

fn repo_and_private_events(ws: &Workspace<FakePorts>) -> (usize, usize) {
    (
        ws.ports().events_in(StoreKind::Repo).len(),
        ws.ports().events_in(StoreKind::Private).len(),
    )
}

#[test]
fn an_unreadable_ground_makes_the_spec_partial_not_invalid_and_cs_writes_nothing() {
    let (ws, finding) = private_ground_world();
    // Where the private store is, the Spec reads everything.
    let here = status(&ws);
    assert!(unreadable(row(&here, "spec/cache-timeout")).is_empty());

    ws.ports().drop_private_store();
    let there = status(&ws);
    let spec = row(&there, "spec/cache-timeout");
    assert!(
        matches!(approval_of(spec), Approval::Draft),
        "partial, not Invalid"
    );
    assert_eq!(unreadable(spec), vec![(finding, finding)]);
    assert!(matches!(spec.next(), NextAction::OpenWithPrivateStore));

    let mut ws = ws;
    let before = repo_and_private_events(&ws);
    let report = block_on(ws.check_cs(&r("spec/cache-timeout"))).expect("check");
    assert_eq!(
        report.violations(),
        [upstream_violation(finding, UpstreamProblem::Unresolved)]
    );
    let refused = block_on(ws.approve_cs(&r("spec/cache-timeout"), None));
    assert!(matches!(
        refused,
        Err(AppError::CheckFailed { gate: Gate::Cs, .. })
    ));
    assert_eq!(repo_and_private_events(&ws), before, "nothing was written");
}

#[test]
fn a_spec_approved_where_the_store_is_makes_its_design_and_item_partial_in_a_clone() {
    let (mut ws, finding) = private_ground_world();
    approve_chain(&mut ws);
    let here = status(&ws);
    for alias in ["spec/cache-timeout", "design/cache"] {
        assert!(unreadable(row(&here, alias)).is_empty(), "{alias}");
    }

    ws.ports().drop_private_store();
    let there = status(&ws);
    let (spec_id, design_id) = (id_of(&ws, "spec/cache-timeout"), id_of(&ws, "design/cache"));
    let spec = row(&there, "spec/cache-timeout");
    let design = row(&there, "design/cache");
    let item = item_row(&there);
    // The approvals are in the repo store, so all three still read as Approved.
    for r in [spec, design, item] {
        assert!(matches!(approval_of(r), Approval::Approved));
    }
    assert_eq!(unreadable(spec), vec![(finding, finding)]);
    assert_eq!(unreadable(design), vec![(finding, spec_id)]);
    assert_eq!(unreadable(item), vec![(finding, design_id)]);

    // CI and CA stop on it, and write nothing.
    let before = repo_and_private_events(&ws);
    let ci = block_on(ws.check_ci(&r("design/cache"))).expect("check");
    assert_eq!(
        ci.violations(),
        [upstream_violation(spec_id, UpstreamProblem::Unresolved)]
    );
    assert!(matches!(
        block_on(ws.approve_ci(&r("design/cache"), None)),
        Err(AppError::CheckFailed { gate: Gate::Ci, .. })
    ));
    let ca = block_on(ws.check_ca(&it("design/cache#1"), None)).expect("check");
    assert_eq!(
        ca.violations(),
        [upstream_violation(design_id, UpstreamProblem::Unresolved)]
    );
    assert!(matches!(
        block_on(ws.approve_ca(&it("design/cache#1"), None, None)),
        Err(AppError::CheckFailed { gate: Gate::Ca, .. })
    ));
    assert_eq!(repo_and_private_events(&ws), before);
}

#[test]
fn stale_and_partial_are_seen_together_and_the_next_step_is_to_open_the_store() {
    let (mut ws, cold) = two_ground_world();
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    block_on(ws.approve_cr(&r("finding/cold-start"), None)).expect("cr");
    block_on(ws.approve_cs(&r("spec/cache-timeout"), None)).expect("cs");
    // The public Finding changes: the Spec is stale.
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 9s");
    block_on(ws.ingest()).expect("ingest");
    ws.ports().drop_private_store();

    let s = status(&ws);
    let spec = row(&s, "spec/cache-timeout");
    assert!(matches!(approval_of(spec), Approval::Approved));
    assert!(matches!(upstream_of(spec), Upstream::Stale(_)));
    assert_eq!(unreadable(spec), vec![(cold, cold)]);
    assert!(matches!(spec.next(), NextAction::OpenWithPrivateStore));

    // The Design under it: the same upstream is Stale and Partial, and the next
    // step is the one, not two.
    let design = row(&s, "design/cache");
    assert!(!matches!(upstream_of(design), Upstream::Ok));
    assert_eq!(
        unreadable(design),
        vec![(cold, id_of(&ws, "spec/cache-timeout"))]
    );
    assert!(matches!(design.next(), NextAction::OpenWithPrivateStore));
}

#[test]
fn a_draft_that_is_partial_asks_to_open_the_store_not_to_approve() {
    let (ws, _) = private_ground_world();
    ws.ports().drop_private_store();
    let s = status(&ws);
    let spec = row(&s, "spec/cache-timeout");
    assert!(matches!(approval_of(spec), Approval::Draft));
    assert!(matches!(spec.next(), NextAction::OpenWithPrivateStore));
    // The Design on it is a Draft of a partial Spec: the same.
    assert!(matches!(
        row(&s, "design/cache").next(),
        NextAction::OpenWithPrivateStore
    ));
}

#[test]
fn another_upstream_that_is_a_public_draft_is_listed_beside_the_partial_one() {
    let (ws, cold) = two_ground_world();
    ws.ports().drop_private_store();
    let public = id_of(&ws, "finding/slow-login");
    let s = status(&ws);
    let spec = row(&s, "spec/cache-timeout");
    // The next step is one, the one that has to come first.
    assert!(matches!(spec.next(), NextAction::OpenWithPrivateStore));
    // The Draft Finding is not hidden by it: the reasons of the state list it.
    let Upstream::Blocked(reasons) = upstream_of(spec) else {
        panic!("blocked by the Draft Finding");
    };
    assert!(
        reasons.iter().any(
            |r| matches!(r, BlockReason::UpstreamUnapproved { upstream } if *upstream == public)
        )
    );
    assert_eq!(unreadable(spec), vec![(cold, cold)]);
}

#[test]
fn a_blocked_entity_names_the_root_of_the_chain_of_the_first_upstream_problem() {
    // Two grounds, each blocking for its own reason: the first listed is a Draft,
    // the second does not parse. The Invalid one comes first in the order of
    // problems, so it is the one the next step names.
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    ws.ports().put_finding("docs/finding/a.md", "A", "a");
    ws.ports().put_finding("docs/finding/b.md", "B", "b");
    add_to(&mut ws, "docs/finding/a.md", "finding/a", StoreKind::Repo);
    add_to(&mut ws, "docs/finding/b.md", "finding/b", StoreKind::Repo);
    ws.ports().put_spec(
        "docs/spec/s.md",
        &["finding/a", "finding/b"],
        "contract",
        &["PIN-TTL"],
    );
    add_to(&mut ws, "docs/spec/s.md", "spec/s", StoreKind::Repo);
    ws.ports()
        .put_broken("docs/finding/b.md", "<<<<<<< ours\nb\n=======\n");
    block_on(ws.ingest()).expect("ingest");

    let s = status(&ws);
    let spec = row(&s, "spec/s");
    let Upstream::Blocked(reasons) = upstream_of(spec) else {
        panic!("blocked");
    };
    assert_eq!(reasons.len(), 2, "both reasons are listed");
    let NextAction::ApproveUpstream { upstream } = spec.next() else {
        panic!("a Blocked entity names an upstream");
    };
    assert_eq!(upstream, &r("finding/b"), "not the Draft listed first");
}

#[test]
fn the_same_id_written_without_the_mark_stays_dangling() {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 4s");
    ws.ports().put_finding("docs/finding/open.md", "Open", "x");
    add_to(
        &mut ws,
        "docs/finding/slow.md",
        "finding/slow-login",
        StoreKind::Private,
    );
    add_to(
        &mut ws,
        "docs/finding/open.md",
        "finding/open",
        StoreKind::Repo,
    );
    let finding = id_of(&ws, "finding/slow-login");
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &["finding/open"],
        "cache the token",
        &["PIN-TTL"],
    );
    add_to(
        &mut ws,
        "docs/spec/cache.md",
        "spec/cache-timeout",
        StoreKind::Repo,
    );
    // A hand edit, not written through the tool: the file now names the private
    // Finding by its bare identifier. No mark: a public document sees only the repo
    // store, with or without the private store being there. (The tool itself refuses
    // to write such a text to the repo store; see the leak tests.)
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &[&finding.to_string()],
        "cache the token",
        &["PIN-TTL"],
    );
    for clone in [false, true] {
        if clone {
            ws.ports().drop_private_store();
        }
        let s = status(&ws);
        let spec = row(&s, "spec/cache-timeout");
        let Approval::Invalid(reasons) = approval_of(spec) else {
            panic!("dangling is Invalid, clone={clone}");
        };
        // An identifier is there, so the Spec is built and is Invalid for the
        // reference that names nothing it can see (not unparsed).
        assert!(
            reasons
                .iter()
                .any(|r| r == &InvalidReason::DanglingRef { upstream: finding }),
            "clone={clone}: {reasons:?}"
        );
        assert!(unreadable(spec).is_empty(), "never partial, clone={clone}");
    }
}

#[test]
fn a_private_id_nothing_holds_is_dangling_where_the_store_is_there_and_partial_where_not() {
    let nothing = format!("private:{}", eid(999));
    // The private store is there (it holds an unrelated Finding).
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    ws.ports()
        .put_finding("docs/finding/other.md", "Other", "x");
    add_to(
        &mut ws,
        "docs/finding/other.md",
        "finding/other",
        StoreKind::Private,
    );
    ws.ports()
        .put_spec("docs/spec/cache.md", &[&nothing], "cache", &["PIN-TTL"]);
    add_to(
        &mut ws,
        "docs/spec/cache.md",
        "spec/cache-timeout",
        StoreKind::Repo,
    );
    let s = status(&ws);
    let spec = row(&s, "spec/cache-timeout");
    assert!(matches!(approval_of(spec), Approval::Invalid(_)));
    assert!(unreadable(spec).is_empty());

    // The private store is not there: the same text is unresolved, not wrong.
    ws.ports().drop_private_store();
    let s = status(&ws);
    let spec = row(&s, "spec/cache-timeout");
    assert!(matches!(approval_of(spec), Approval::Draft));
    assert_eq!(unreadable(spec).len(), 1);
}

#[test]
fn resolving_a_private_reference_in_a_clone_is_unresolved_not_missing() {
    let (ws, finding) = private_ground_world();
    let reference = Ref::Private(finding);
    let model = block_on(Model::load(ws.ports(), ws.config())).expect("model");
    let (id, kind, store) = model.resolve(&reference).expect("found where it is");
    assert_eq!(
        (id, kind, store),
        (finding, EntityKind::Finding, StoreKind::Private)
    );

    ws.ports().drop_private_store();
    let model = block_on(Model::load(ws.ports(), ws.config())).expect("model");
    assert!(matches!(
        model.resolve(&reference),
        Err(AppError::Unresolved { reference: found }) if found == reference
    ));
    // A command that needs the entity says so, and `get` shows it.
    assert!(matches!(
        block_on(ws.check_cs(&reference)),
        Err(AppError::Unresolved { .. })
    ));
    assert!(matches!(
        block_on(ws.get(&Target::Entity(reference))).expect("get"),
        GetResult::Unresolved(Ref::Private(_))
    ));
}

// --- a text for the repo store names a private entity only by its mark -----------

/// A private Finding, and the workspace it is in.
fn private_finding_world() -> (Workspace<FakePorts>, EntityId) {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 4s");
    add_to(
        &mut ws,
        "docs/finding/slow.md",
        "finding/slow-login",
        StoreKind::Private,
    );
    let finding = id_of(&ws, "finding/slow-login");
    (ws, finding)
}

fn unmarked(finding: EntityId) -> Ref {
    Ref::Local(LocalRef::Id(finding))
}

#[test]
fn a_document_that_names_a_private_entity_without_the_mark_cannot_be_added_to_the_repo_store() {
    let (mut ws, finding) = private_finding_world();
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &[&finding.to_string()],
        "cache the token",
        &["PIN-TTL"],
    );
    let options = AddOptions::new()
        .with_alias(a("spec/cache-timeout"))
        .with_store(StoreKind::Repo);
    let before = repo_and_private_events(&ws);
    let refused = block_on(ws.add(&[path("docs/spec/cache.md")], &options));
    let Err(AppError::LeakFound { findings }) = refused else {
        panic!("expected the leak lint to object, found {refused:?}");
    };
    assert_eq!(
        findings
            .iter()
            .map(|f| f.unmarked_reference().cloned())
            .collect::<Vec<_>>(),
        [Some(unmarked(finding))]
    );
    assert_eq!(repo_and_private_events(&ws), before, "nothing was written");

    // The opaque identifier is how a public document names a private entity.
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &[&format!("private:{finding}")],
        "cache the token",
        &["PIN-TTL"],
    );
    block_on(ws.add(&[path("docs/spec/cache.md")], &options)).expect("add with the mark");
}

#[test]
fn publish_needs_the_mark_on_a_private_reference_and_goes_on_with_it() {
    let config = test_config().with_default_store(StoreKind::Private);
    let mut ws = Workspace::new(FakePorts::new(), config);
    ws.ports()
        .put_finding("docs/finding/slow.md", "Slow login", "p95 is 4s");
    block_on(ws.add(
        &[path("docs/finding/slow.md")],
        &AddOptions::new().with_alias(a("finding/slow-login")),
    ))
    .expect("add");
    let finding = id_of(&ws, "finding/slow-login");
    // Private to private, the bare identifier is fine.
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &[&finding.to_string()],
        "cache the token",
        &["PIN-TTL"],
    );
    block_on(ws.add(
        &[path("docs/spec/cache.md")],
        &AddOptions::new().with_alias(a("spec/cache-timeout")),
    ))
    .expect("add");

    // Published, it would be the repo store's text: the bare identifier is stopped.
    let refused = block_on(ws.publish(&r("spec/cache-timeout")));
    let Err(AppError::LeakFound { findings }) = refused else {
        panic!("expected the leak lint to object, found {refused:?}");
    };
    assert_eq!(findings.len(), 1);
    assert!(ws.ports().events_in(StoreKind::Repo).is_empty());

    // Written with the mark, recorded, it can go.
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &[&format!("private:{finding}")],
        "cache the token",
        &["PIN-TTL"],
    );
    block_on(ws.ingest()).expect("ingest");
    block_on(ws.publish(&r("spec/cache-timeout"))).expect("publish with the mark");
}

#[test]
fn a_scaffold_in_the_repo_store_names_a_private_upstream_the_way_the_domain_says() {
    let (mut ws, finding) = private_finding_world();
    // The new file is public (the default store), the Finding is private: the mark
    // is asked of the store rule, and a file written that way passes the gate.
    assert_eq!(
        StoreKind::Repo.mark_for(StoreKind::Private),
        crate::domain::event::RefMark::Private
    );
    block_on(ws.spec_new(a("spec/cache-timeout"), &r("finding/slow-login"))).expect("spec new");
    let s = status(&ws);
    let spec = row(&s, "spec/cache-timeout");
    // The Spec is built on the Finding (its template does not parse as a Spec yet, but
    // the reference it names is the mark and not a name that leaks).
    assert!(
        !matches!(
            approval_of(spec),
            Approval::Invalid(reasons) if reasons.iter().any(|reason| matches!(
                reason,
                InvalidReason::DanglingRef { upstream } if *upstream == finding
            ))
        ),
        "the Finding is found by its opaque identifier"
    );
}

// --- the reasons a reference can come to ---------------------------------------

#[test]
fn an_alias_that_names_nothing_or_several_is_unparsed_and_a_bare_identifier_is_dangling() {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    ws.ports()
        .put_spec("docs/spec/a.md", &["finding/nothing"], "c", &["PIN-A"]);
    add_to(&mut ws, "docs/spec/a.md", "spec/a", StoreKind::Repo);
    let nothing = eid(999);
    ws.ports()
        .put_spec("docs/spec/b.md", &[&nothing.to_string()], "c", &["PIN-A"]);
    add_to(&mut ws, "docs/spec/b.md", "spec/b", StoreKind::Repo);

    // An alias that names nothing has no identifier to keep: the file does not build.
    let s = status(&ws);
    let Approval::Invalid(reasons) = approval_of(row(&s, "spec/a")) else {
        panic!("expected Invalid");
    };
    assert_eq!(
        reasons.iter().collect::<Vec<_>>(),
        [&InvalidReason::Unparsed(UnparsedReason::DanglingRef {
            reference: r("finding/nothing")
        })]
    );
    // An identifier is kept: the Spec is built and the evaluator finds nothing under it.
    let Approval::Invalid(reasons) = approval_of(row(&s, "spec/b")) else {
        panic!("expected Invalid");
    };
    assert_eq!(
        reasons.iter().collect::<Vec<_>>(),
        [&InvalidReason::DanglingRef { upstream: nothing }]
    );
}

#[test]
fn an_alias_held_in_each_store_is_ambiguous_for_a_private_document() {
    let config = test_config().with_default_store(StoreKind::Private);
    let mut ws = Workspace::new(FakePorts::new(), config);
    ws.ports().put_finding("docs/finding/one.md", "One", "x");
    ws.ports().put_finding("docs/finding/two.md", "Two", "y");
    add_to(
        &mut ws,
        "docs/finding/one.md",
        "finding/dup",
        StoreKind::Private,
    );
    add_to(
        &mut ws,
        "docs/finding/two.md",
        "finding/dup",
        StoreKind::Repo,
    );
    ws.ports()
        .put_spec("docs/spec/a.md", &["finding/dup"], "c", &["PIN-A"]);
    add_to(&mut ws, "docs/spec/a.md", "spec/a", StoreKind::Private);
    let s = status(&ws);
    let Approval::Invalid(reasons) = approval_of(row(&s, "spec/a")) else {
        panic!("expected Invalid");
    };
    assert_eq!(
        reasons.iter().collect::<Vec<_>>(),
        [&InvalidReason::Unparsed(UnparsedReason::AmbiguousRef {
            reference: r("finding/dup")
        })]
    );
}

#[test]
fn a_reference_to_an_entity_of_another_kind_is_told_with_both_kinds() {
    let mut ws = workspace();
    ws.ports().put_spec(
        "docs/spec/cache.md",
        &["design/cache"],
        "cache the token",
        &["PIN-TTL"],
    );
    block_on(ws.ingest()).expect("ingest");
    let s = status(&ws);
    let Approval::Invalid(reasons) = approval_of(row(&s, "spec/cache-timeout")) else {
        panic!("expected Invalid");
    };
    assert_eq!(
        reasons.iter().collect::<Vec<_>>(),
        [&InvalidReason::Unparsed(UnparsedReason::WrongKind {
            reference: r("design/cache"),
            expected: EntityKind::Finding,
            found: EntityKind::Design,
        })]
    );
}

// --- Invalid is a violation of every gate --------------------------------------

#[test]
fn an_invalid_spec_gets_a_violation_at_cs_and_at_cm_and_nothing_is_written() {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    let nothing = eid(999);
    ws.ports()
        .put_spec("docs/spec/b.md", &[&nothing.to_string()], "c", &["PIN-A"]);
    add_to(&mut ws, "docs/spec/b.md", "spec/b", StoreKind::Repo);
    let dangling = InvalidReason::DanglingRef { upstream: nothing };
    let invalid_target = |violations: &[CheckViolation]| matches!(violations, [CheckViolation::TargetInvalid { reasons }] if reasons.first() == &dangling);

    let before = repo_and_private_events(&ws);
    // The machine layer says it as a violation, not as an error.
    let report = block_on(ws.check_cs(&r("spec/b"))).expect("a report");
    assert!(
        invalid_target(report.violations()),
        "{:?}",
        report.violations()
    );
    // The measurement is a gate too: it records nothing over an Invalid Spec.
    let refused = block_on(ws.record_cm(&r("spec/b"), &CmInput::Pass, None));
    let Err(AppError::CheckFailed {
        gate: Gate::Cm,
        violations,
    }) = refused
    else {
        panic!("expected a refused CM, found {refused:?}");
    };
    assert!(invalid_target(&violations), "{violations:?}");
    let refused = block_on(ws.approve_cs(&r("spec/b"), None));
    assert!(matches!(
        refused,
        Err(AppError::CheckFailed { gate: Gate::Cs, .. })
    ));
    assert_eq!(repo_and_private_events(&ws), before, "nothing was recorded");
}

#[test]
fn the_cs_a_measurement_is_set_against_may_have_been_recorded_before_two_registrations_merged() {
    let mut ws = workspace();
    approve_chain(&mut ws);
    let spec = id_of(&ws, "spec/cache-timeout");
    // A branch registered the same Spec file again under another identifier; the
    // merge keeps the smaller one as the Spec, and CS approvals recorded against the
    // other still count for it.
    let events = ws.ports().events_in(StoreKind::Repo);
    let registration = events
        .iter()
        .find(|e| e.target() == spec && matches!(e.body(), EventBody::Registered(_)))
        .expect("the registration");
    let EventBody::Registered(Registration::Doc(doc)) = registration.body() else {
        panic!("a document registration");
    };
    let survivor = eid(0);
    let twin = Event::new(
        survivor,
        BTreeSet::new(),
        registration.at().clone(),
        EventBody::Registered(Registration::Doc(doc.clone())),
    )
    .expect("event");
    block_on(StorePort::append(ws.ports(), StoreKind::Repo, &twin)).expect("append");
    // The smaller identifier survives, and the Spec's approvals are asked for it.
    let model = block_on(Model::load(ws.ports(), ws.config())).expect("model");
    assert_eq!(model.canonical(spec), survivor);
    block_on(ws.record_cm(&r("spec/cache-timeout"), &CmInput::Pass, None)).expect("cm");
}

// --- a document two branches made of different kinds --------------------------

/// Two branches each add an event to the stream from the tips both saw.
fn fork(ws: &Workspace<FakePorts>, id: EntityId, bodies: [EventBody; 2]) {
    let tips = block_on(Model::load(ws.ports(), ws.config()))
        .expect("model")
        .tips_in(StoreKind::Repo, id);
    let at = ws.ports().events_in(StoreKind::Repo)[0].at().clone();
    for body in bodies {
        let event = Event::new(id, tips.clone(), at.clone(), body).expect("event");
        block_on(StorePort::append(ws.ports(), StoreKind::Repo, &event)).expect("append");
    }
}

fn set_kind(kind: DocKind) -> EventBody {
    EventBody::Reclassified {
        changes: NonEmptyVec::new(ClassChange::SetKind(kind), Vec::new()),
    }
}

#[test]
fn a_kind_set_differently_on_two_branches_is_invalid_of_no_kind_and_blocks_what_stands_on_it() {
    let mut ws = workspace();
    let finding = id_of(&ws, "finding/slow-login");
    fork(
        &ws,
        finding,
        [set_kind(DocKind::Spec), set_kind(DocKind::Design)],
    );
    let kinds = BTreeSet::from([DocKind::Spec, DocKind::Design]);

    let s = status(&ws);
    let conflicted = row(&s, "finding/slow-login");
    assert_eq!(conflicted.kind(), &RowKind::Conflict(kinds));
    assert!(matches!(
        approval_of(conflicted),
        Approval::Invalid(reasons) if reasons.first()
            == &InvalidReason::ClassificationConflict { field: ConflictField::Kind }
    ));
    assert!(s.issues().iter().any(|issue| matches!(
        &issue.issue,
        crate::domain::classification::ClassificationIssue::Conflict {
            field: ConflictField::Kind,
            ..
        }
    )));
    // It is made into an entity of neither kind.
    let model = block_on(Model::load(ws.ports(), ws.config())).expect("model");
    assert!(!model.findings().contains_key(&FindingId::new(finding)));
    assert!(!model.specs().contains_key(&SpecId::new(finding)));
    assert!(!model.designs().contains_key(&DesignId::new(finding)));

    // What stands on it is Blocked by an invalid upstream, and the gate says so.
    let spec = row(&s, "spec/cache-timeout");
    assert!(matches!(
        upstream_of(spec),
        Upstream::Blocked(reasons)
            if reasons.first() == &BlockReason::UpstreamInvalid { upstream: finding }
    ));
    let report = block_on(ws.check_cs(&r("spec/cache-timeout"))).expect("check");
    assert_eq!(
        report.violations(),
        [upstream_violation(finding, UpstreamProblem::Invalid)]
    );
    assert!(matches!(
        block_on(ws.approve_cs(&r("spec/cache-timeout"), None)),
        Err(AppError::CheckFailed { gate: Gate::Cs, .. })
    ));
    // The document itself is a violation of the gate for a kind it might be.
    let report = block_on(ws.check_cs(&r("finding/slow-login"))).expect("check");
    assert!(matches!(
        report.violations(),
        [CheckViolation::TargetInvalid { reasons }]
            if reasons.first() == &InvalidReason::ClassificationConflict { field: ConflictField::Kind }
    ));

    // Setting the kind again settles it: the new event lists both branches.
    block_on(ws.reclassify(
        &r("finding/slow-login"),
        NonEmptyVec::new(ClassChange::SetKind(DocKind::Finding), Vec::new()),
    ))
    .expect("settle");
    let s = status(&ws);
    let settled = row(&s, "finding/slow-login");
    assert_eq!(settled.kind(), &RowKind::Entity(EntityKind::Finding));
    assert!(matches!(approval_of(settled), Approval::Draft));
    assert!(s.issues().is_empty());
}

#[test]
fn publish_purge_and_the_other_reads_of_a_conflicted_classification_are_stopped() {
    let config = test_config().with_default_store(StoreKind::Private);
    let mut ws = Workspace::new(files(), config);
    register_chain(&mut ws);
    let finding = id_of(&ws, "finding/slow-login");
    let events = ws.ports().events_in(StoreKind::Private);
    let tips = block_on(Model::load(ws.ports(), ws.config()))
        .expect("model")
        .tips_in(StoreKind::Private, finding);
    for kind in [DocKind::Spec, DocKind::Design] {
        let event = Event::new(
            finding,
            tips.clone(),
            events[0].at().clone(),
            set_kind(kind),
        )
        .expect("event");
        block_on(StorePort::append(ws.ports(), StoreKind::Private, &event)).expect("append");
    }
    let before = repo_and_private_events(&ws);
    let stopped = |result: Result<_, AppError>| {
        matches!(
            result,
            Err(AppError::ClassificationConflict {
                field: ConflictField::Kind,
                ..
            })
        )
    };
    assert!(stopped(
        block_on(ws.publish(&r("finding/slow-login"))).map(|_| ())
    ));
    let raw = events
        .iter()
        .find_map(|e| match e.body() {
            EventBody::Registered(Registration::Doc(d)) if e.target() == finding => {
                Some(*d.first().raw())
            }
            _ => None,
        })
        .expect("a version");
    assert!(stopped(
        block_on(ws.purge(&r("finding/slow-login"), raw, why("a secret"))).map(|_| ())
    ));
    assert_eq!(repo_and_private_events(&ws), before, "nothing was written");
}

#[test]
fn a_path_set_differently_on_two_branches_is_invalid_and_a_move_settles_it() {
    let mut ws = workspace();
    let finding = id_of(&ws, "finding/slow-login");
    let old = path("docs/finding/slow.md");
    let moved_to = |to: &str| EventBody::Moved {
        old: old.clone(),
        new: path(to),
    };
    fork(
        &ws,
        finding,
        [moved_to("docs/finding/a.md"), moved_to("docs/finding/b.md")],
    );
    let s = status(&ws);
    let conflicted = row(&s, "finding/slow-login");
    assert_eq!(
        conflicted.kind(),
        &RowKind::Conflict(BTreeSet::from([DocKind::Finding]))
    );
    assert!(matches!(
        approval_of(conflicted),
        Approval::Invalid(reasons) if reasons.first()
            == &InvalidReason::ClassificationConflict { field: ConflictField::Path }
    ));
    // The file is not read: which one it is, is not known.
    let spec = row(&s, "spec/cache-timeout");
    assert!(matches!(upstream_of(spec), Upstream::Blocked(_)));

    ws.ports()
        .put_finding("docs/finding/a.md", "Slow login", "p95 is 4s");
    block_on(ws.mv(&r("finding/slow-login"), path("docs/finding/a.md"))).expect("mv settles");
    let s = status(&ws);
    assert_eq!(
        row(&s, "finding/slow-login").kind(),
        &RowKind::Entity(EntityKind::Finding)
    );
}

/// A workspace with a Finding at `docs/finding/slow.md` whose kind two branches set
/// differently, and a second Finding at `docs/finding/lonely.md`.
fn conflicted_and_lonely() -> (Workspace<FakePorts>, EntityId) {
    let mut ws = workspace();
    ws.ports()
        .put_finding("docs/finding/lonely.md", "Lonely", "nobody cites me");
    block_on(ws.add(
        &[path("docs/finding/lonely.md")],
        &AddOptions::new().with_alias(a("finding/lonely")),
    ))
    .expect("add");
    let finding = id_of(&ws, "finding/slow-login");
    fork(
        &ws,
        finding,
        [set_kind(DocKind::Finding), set_kind(DocKind::Spec)],
    );
    (ws, finding)
}

#[test]
fn a_path_held_by_a_document_in_conflict_is_taken_for_add_and_mv() {
    let (mut ws, held) = conflicted_and_lonely();

    // A second registration at the same path is not made.
    let added = block_on(ws.add(&[path("docs/finding/slow.md")], &AddOptions::new())).expect("add");
    assert!(added.registered().is_empty(), "{:?}", added.registered());
    assert_eq!(added.skipped(), [path("docs/finding/slow.md")]);

    // Nor does a move take it.
    let refused = block_on(ws.mv(&r("finding/lonely"), path("docs/finding/slow.md")));
    assert!(
        matches!(refused, Err(AppError::PathOccupied { holder, .. }) if holder == held),
        "{refused:?}"
    );
}

#[test]
fn every_path_a_document_in_conflict_may_be_at_is_taken() {
    let mut ws = workspace();
    ws.ports()
        .put_finding("docs/finding/lonely.md", "Lonely", "nobody cites me");
    block_on(ws.add(
        &[path("docs/finding/lonely.md")],
        &AddOptions::new().with_alias(a("finding/lonely")),
    ))
    .expect("add");
    let finding = id_of(&ws, "finding/slow-login");
    let old = path("docs/finding/slow.md");
    let moved_to = |to: &str| EventBody::Moved {
        old: old.clone(),
        new: path(to),
    };
    fork(
        &ws,
        finding,
        [moved_to("docs/finding/a.md"), moved_to("docs/finding/b.md")],
    );
    ws.ports()
        .put_finding("docs/finding/a.md", "Slow login", "p95 is 4s");
    let refused = block_on(ws.mv(&r("finding/lonely"), path("docs/finding/a.md")));
    assert!(
        matches!(refused, Err(AppError::PathOccupied { holder, .. }) if holder == finding),
        "{refused:?}"
    );
}

#[test]
fn a_file_at_the_path_of_a_document_in_conflict_is_not_a_move_of_another() {
    let (mut ws, _) = conflicted_and_lonely();
    // `lonely` vanishes, and a file with its very text appears where the document in
    // conflict is: that is not where `lonely` went.
    ws.ports().remove_file("docs/finding/lonely.md");
    ws.ports()
        .put_finding("docs/finding/slow.md", "Lonely", "nobody cites me");
    let report = block_on(ws.ingest()).expect("ingest");
    assert!(report.moved().is_empty(), "{:?}", report.moved());
    assert_eq!(report.missing(), [path("docs/finding/lonely.md")]);
}

#[test]
fn an_alias_set_differently_on_two_branches_is_an_issue_and_finds_nothing() {
    let ws = workspace();
    let spec = id_of(&ws, "spec/cache-timeout");
    let set_alias = |alias: &str| EventBody::Reclassified {
        changes: NonEmptyVec::new(ClassChange::SetAlias(a(alias)), Vec::new()),
    };
    fork(&ws, spec, [set_alias("spec/one"), set_alias("spec/two")]);
    let model = block_on(Model::load(ws.ports(), ws.config())).expect("model");
    for alias in ["spec/one", "spec/two", "spec/cache-timeout"] {
        assert!(
            matches!(model.resolve(&r(alias)), Err(AppError::NotFound { .. })),
            "{alias} finds nothing"
        );
    }
    assert!(model.doc(spec).expect("the document").alias().is_none());
    assert!(model.issues(StoreKind::Repo).iter().any(|issue| matches!(
        issue,
        crate::domain::classification::ClassificationIssue::Conflict {
            field: ConflictField::Alias,
            ..
        }
    )));
}

// --- the next step of an item names the item -----------------------------------

#[test]
fn the_next_step_of_an_item_names_the_item_by_its_parts() {
    let mut ws = workspace();
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    block_on(ws.approve_cs(&r("spec/cache-timeout"), None)).expect("cs");
    block_on(ws.approve_ci(&r("design/cache"), None)).expect("ci");
    let s = status(&ws);
    assert_eq!(
        item_row(&s).next(),
        &NextAction::Approve {
            gate: Gate::Ca,
            target: Target::Item(it("design/cache#1")),
        }
    );
    // Its text is `<ref>#<n>`.
    assert_eq!(
        item_row(&s).next().to_string(),
        "whyshelf check ca design/cache#1"
    );
}

#[test]
fn a_published_finding_named_by_private_mark_is_found_in_a_clone_and_the_spec_is_complete() {
    let (mut ws, finding) = private_ground_world();
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    block_on(ws.publish(&r("finding/slow-login"))).expect("publish");
    ws.ports().drop_private_store();

    let s = status(&ws);
    let spec = row(&s, "spec/cache-timeout");
    assert!(
        unreadable(spec).is_empty(),
        "found by its id in the repo store"
    );
    // The approval stayed private, so the Finding is a Draft here: the Spec is
    // blocked by it, not unresolved.
    assert!(matches!(upstream_of(spec), Upstream::Blocked(_)));
    assert!(matches!(spec.next(), NextAction::ApproveUpstream { .. }));
    let report = block_on(ws.check_cs(&r("spec/cache-timeout"))).expect("check");
    assert_eq!(
        report.violations(),
        [upstream_violation(finding, UpstreamProblem::NotApproved)]
    );
}

// --- stage 2: pins, classification keys, integration, test runs ---------------

/// The chain approved up to CI, so that only CA is left to try.
fn ready_for_ca() -> Workspace<FakePorts> {
    let mut ws = workspace();
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    block_on(ws.approve_cs(&r("spec/cache-timeout"), None)).expect("cs");
    block_on(ws.approve_ci(&r("design/cache"), None)).expect("ci");
    ws
}

#[test]
fn a_tag_that_is_no_pin_name_and_a_timed_out_run_stop_ca_and_nothing_is_recorded() {
    use crate::domain::lint::CheckViolation as V;
    let ws = ready_for_ca();
    ws.ports().set_malformed(&[("tests/t.rs", 9, "PIN-foo")]);
    ws.ports().set_tests_time_out(true);
    let report = block_on(ws.check_ca(&it("design/cache#1"), None)).expect("check");
    assert!(report.violations().iter().any(
        |v| matches!(v, V::MalformedPin { location, .. } if location.to_string() == "tests/t.rs:9")
    ));
    let limit = super::config::DEFAULT_TEST_TIMEOUT;
    assert!(
        report
            .violations()
            .contains(&V::TestsTimedOut { after: limit })
    );

    let mut ws = ws;
    let before = ws.ports().events_in(StoreKind::Repo).len();
    assert!(matches!(
        block_on(ws.approve_ca(&it("design/cache#1"), None, None)),
        Err(AppError::CheckFailed { gate: Gate::Ca, .. })
    ));
    assert_eq!(ws.ports().events_in(StoreKind::Repo).len(), before);

    // Without the mistakes the same item is approved.
    ws.ports().set_malformed(&[]);
    ws.ports().set_tests_time_out(false);
    block_on(ws.approve_ca(&it("design/cache#1"), None, None)).expect("ca");
}

#[test]
fn each_of_two_specs_gets_its_ca_when_the_tests_carry_the_pins_of_both() {
    let mut ws = workspace();
    ws.ports().put_spec(
        "docs/spec/other.md",
        &["finding/slow-login"],
        "something else",
        &["PIN-OTHER"],
    );
    ws.ports().put_design(
        "docs/design/other.md",
        "spec/other-spec",
        &[(1, "do the other", &["PIN-OTHER"])],
    );
    let opts = |alias: &str| AddOptions::new().with_alias(a(alias));
    block_on(ws.add(&[path("docs/spec/other.md")], &opts("spec/other-spec"))).expect("add");
    block_on(ws.add(&[path("docs/design/other.md")], &opts("design/other"))).expect("add");
    block_on(ws.approve_cr(&r("finding/slow-login"), None)).expect("cr");
    for spec in ["spec/cache-timeout", "spec/other-spec"] {
        block_on(ws.approve_cs(&r(spec), None)).expect("cs");
    }
    for design in ["design/cache", "design/other"] {
        block_on(ws.approve_ci(&r(design), None)).expect("ci");
    }
    // The tests of the project carry both Specs' pins.
    ws.ports().set_pins(&["PIN-TTL", "PIN-OTHER"]);
    for item in ["design/cache#1", "design/other#1"] {
        block_on(ws.approve_ca(&it(item), None, None)).unwrap_or_else(|e| panic!("{item}: {e:?}"));
    }
}

#[test]
fn a_clone_that_cannot_tell_integration_shows_unknown_and_no_command_stops() {
    use crate::domain::revision::UnknownIntegration;
    let mut ws = workspace();
    approve_chain(&mut ws);
    ws.ports()
        .cannot_tell_integration(Some(UnknownIntegration::BaselineUnresolved));
    let s = status(&ws);
    assert!(matches!(
        item_row(&s).integration(),
        Some(Integration::Unknown {
            reason: UnknownIntegration::BaselineUnresolved,
            ..
        })
    ));
    // Nothing is due that depends on the baseline, and the other reads go on.
    assert!(!matches!(
        row(&s, "spec/cache-timeout").next(),
        NextAction::AwaitingCm { .. }
    ));
    block_on(ws.ingest()).expect("ingest");
    block_on(ws.trace(&r("design/cache"))).expect("trace");
    // Once the clone has the baseline, it is told.
    ws.ports().cannot_tell_integration(None);
    ws.ports().integrate("rev-1");
    assert!(matches!(
        item_row(&status(&ws)).integration(),
        Some(Integration::Integrated { .. })
    ));
}

fn file_keys(tags: &[&str], owner: &str, alias: &str) -> crate::domain::front::FileClassification {
    crate::domain::front::FileClassification {
        tags: Some(tags.iter().map(|t| t.parse().expect("a tag")).collect()),
        kind: Some(DocKind::Finding),
        owner: Some(crate::domain::actor::Actor::new(owner).expect("a name")),
        alias: Some(a(alias)),
    }
}

#[test]
fn front_matter_classification_is_taken_in_at_registration_and_a_later_difference_warns() {
    use crate::domain::classification::ClassificationIssue;
    let mut ws = Workspace::new(files(), test_config());
    ws.ports().set_file_classification(
        "docs/finding/slow.md",
        file_keys(&["area:auth"], "bob", "finding/from-file"),
    );
    block_on(ws.add(&[path("docs/finding/slow.md")], &AddOptions::new())).expect("add");

    // Taken in once: found by the alias the file gave, tagged, owned.
    let GetResult::Entity(view) =
        block_on(ws.get(&Target::Entity(r("finding/from-file")))).expect("get")
    else {
        panic!("an entity");
    };
    assert_eq!(view.tags().len(), 1);
    assert_eq!(view.owner().map(|o| o.as_str()), Some("bob"));
    assert!(
        status(&ws).issues().is_empty(),
        "the file and the store agree"
    );

    // The store is the truth afterwards: a tag taken off there leaves the file saying
    // something else, which is shown and nothing more.
    block_on(ws.tag(&r("finding/from-file"), add_tag("-area:auth"))).expect("untag");
    let s = status(&ws);
    assert!(matches!(
        s.issues(),
        [issue] if matches!(&issue.issue, ClassificationIssue::FileDiffers { drift, .. }
            if drift.field == crate::domain::front::ClassField::Tags)
    ));
    // Ingest does not take the file's tags in a second time.
    block_on(ws.ingest()).expect("ingest");
    let GetResult::Entity(view) =
        block_on(ws.get(&Target::Entity(r("finding/from-file")))).expect("get")
    else {
        panic!("an entity");
    };
    assert!(view.tags().is_empty());
}

#[test]
fn an_alias_or_tag_the_registration_cannot_take_in_is_left_out_and_shown_as_a_difference() {
    let mut ws = workspace();
    ws.ports().put_finding("docs/finding/b.md", "Other", "text");
    ws.ports().set_file_classification(
        "docs/finding/b.md",
        file_keys(&["area:nothing"], "bob", "finding/slow-login"),
    );
    block_on(ws.add(&[path("docs/finding/b.md")], &AddOptions::new())).expect("add");
    let s = status(&ws);
    // The alias is held by another document and the tag is outside the vocabulary:
    // neither blocks the registration, both are shown.
    assert_eq!(s.issues().len(), 2);
}

#[test]
fn two_files_of_one_add_cannot_both_take_the_alias_their_keys_say() {
    let mut ws = Workspace::new(FakePorts::new(), test_config());
    for (file, title) in [
        ("docs/finding/a.md", "First"),
        ("docs/finding/b.md", "Second"),
    ] {
        ws.ports().put_finding(file, title, "text");
        ws.ports()
            .set_file_classification(file, file_keys(&[], "bob", "finding/x"));
    }
    block_on(ws.add(
        &[path("docs/finding/a.md"), path("docs/finding/b.md")],
        &AddOptions::new(),
    ))
    .expect("add");
    // Only one holds the alias; the other left it out and shows it as a difference.
    let holders = ws
        .ports()
        .events_in(StoreKind::Repo)
        .iter()
        .filter(|e| {
            matches!(e.body(), EventBody::Registered(Registration::Doc(d))
                if d.alias().is_some_and(|alias| alias.as_str() == "finding/x"))
        })
        .count();
    assert_eq!(holders, 1);
    assert_eq!(status(&ws).issues().len(), 1);
}

#[test]
fn a_file_whose_keys_name_no_single_kind_asks_for_a_kind() {
    use crate::domain::front::Guess;
    let mut ws = Workspace::new(files(), test_config());
    ws.ports().put_broken("docs/odd.md", "text");
    ws.ports().set_file_guess("docs/odd.md", Guess::Several);
    let error = block_on(ws.add(&[path("docs/odd.md")], &AddOptions::new())).unwrap_err();
    assert!(matches!(
        error,
        AppError::KindUnknown {
            guess: Guess::Several,
            ..
        }
    ));
    assert!(error.to_string().contains("--kind"));
    assert!(ws.ports().events_in(StoreKind::Repo).is_empty());
    // With a kind given, the file is registered (as an Invalid document).
    block_on(ws.add(
        &[path("docs/odd.md")],
        &AddOptions::new().with_kind(DocKind::Finding),
    ))
    .expect("add");
}
