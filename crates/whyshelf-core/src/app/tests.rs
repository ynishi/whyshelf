//! The use cases, end to end, on the in-memory ports.
//!
//! Every test builds a `Workspace<FakePorts>`, drives it with `block_on`, and looks
//! at the result of the use case, at the events in the fake store, or at both.

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::approval::{Gate, TestOutcome};
use crate::domain::classification::fold;
use crate::domain::event::{
    ClassChange, EntityKind, Event, EventBody, Registration, StoreKind, Version,
};
use crate::domain::finding::FindingType;
use crate::domain::id::{Alias, EntityId, test_support::eid};
use crate::domain::lint::CheckViolation;
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::query::Query;
use crate::domain::reference::{ItemRef, Ref};
use crate::domain::state::{Approval, Integration, Upstream};
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
use super::view::{ExportProblem, NextAction, StatusReport, StatusRow};

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
        .find(|r| r.kind() == EntityKind::Item)
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
    block_on(ws.approve_ca(&it("design/cache#1"), Some(&older), None)).expect("ca");
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
    assert!(matches!(wrong, Err(AppError::CmFindingNotMeasured { .. })));

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
            .filter(|r| r.kind() == EntityKind::Item)
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
    assert!(!item.approved()[0].integrated);

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
