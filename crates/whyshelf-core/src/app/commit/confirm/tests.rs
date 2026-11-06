use super::*;
use crate::app::commit::fixtures::{Publishing, ada};
use crate::app::fake::{FakeCorpus, Reply, block_on};
use crate::app::ports::{ConfirmAction, PlanSummary};
use crate::domain::judge::test_support::agent;
use crate::domain::judge::{PersonName, Principal};
use crate::domain::publication::ConfirmDigest;

fn prompt_for(publishing: &Publishing) -> ConfirmPrompt {
    ConfirmPrompt::of_publication(&publishing.plan).expect("a publication needs a confirmation")
}

fn undeclared() -> Judge {
    Judge::new(
        PersonName::new("ada").expect("a name"),
        None,
        Principal::Undeclared,
    )
}

#[test]
fn a_publication_asks_for_the_digest_of_its_plan_and_says_what_it_does() {
    let publishing = Publishing::of(b"bytes");
    let prompt = prompt_for(&publishing);
    assert_eq!(prompt.digest(), plan_digest(&publishing));
    assert_eq!(
        prompt.summary(),
        PlanSummary::new(ConfirmAction::Publish, publishing.plan.records().len())
    );
    assert_eq!(prompt.digest().short().len(), 8);
}

fn plan_digest(publishing: &Publishing) -> ConfirmDigest {
    ConfirmDigest::Plan(publishing.plan.digest())
}

#[test]
fn a_plan_that_asks_for_nothing_has_no_prompt() {
    // The private copy to remove says what the public file does.
    let plan = Publishing::recorded(b"bytes").plan;
    assert_eq!(plan.confirmation(), None);
    assert_eq!(ConfirmPrompt::of_publication(&plan), None);

    // One that says something else is lost with it: asked, and for another reason
    // than the records of a new publication.
    let edited = Publishing::recorded(b"edited since").plan;
    let prompt = ConfirmPrompt::of_publication(&edited).expect("a differing copy asks");
    assert_eq!(
        prompt.summary(),
        PlanSummary::new(ConfirmAction::RemoveDifferingCopy, 1)
    );
}

#[test]
fn an_abort_and_an_adoption_ask_for_the_digest_of_what_they_do() {
    use crate::domain::adoption;
    let adoptable = crate::app::commit::fixtures::adoptable_finding();
    let prompt = ConfirmPrompt::of_adoption(&adoptable).expect("an adoption always asks");
    assert_eq!(
        prompt.digest(),
        ConfirmDigest::Adopt(adoption::digest(&adoptable))
    );
    assert_eq!(prompt.summary(), PlanSummary::new(ConfirmAction::Adopt, 1));

    // An abort removes a file that is a copy of the private one, so it asks for
    // nothing: a copy that differs belongs to a recorded publication, which is finished.
    let copy = crate::app::commit::fixtures::Aborting::of(b"bytes");
    assert_eq!(ConfirmPrompt::of_abort(&copy.plan), None);
}

#[test]
fn a_yes_to_a_prompt_is_a_confirmation_of_that_plan() {
    let publishing = Publishing::of(b"bytes");
    let prompt = prompt_for(&publishing);
    let fake = FakeCorpus::new();
    fake.set_reply(Reply::Always(Confirmed::Yes));

    let confirmation = block_on(confirm(&fake, &ada(), &prompt)).expect("confirmed");
    assert_eq!(confirmation.digest(), plan_digest(&publishing));
    assert_eq!(fake.asked(), vec![prompt]);
}

#[test]
fn a_person_who_types_back_the_digest_confirms_and_one_who_types_another_does_not() {
    let publishing = Publishing::of(b"bytes");
    let prompt = prompt_for(&publishing);
    let fake = FakeCorpus::new();

    fake.set_reply(Reply::TypesBack(plan_digest(&publishing)));
    assert!(block_on(confirm(&fake, &ada(), &prompt)).is_ok());

    let other = Publishing::of(b"other bytes");
    assert_ne!(other.plan.digest(), publishing.plan.digest());
    fake.set_reply(Reply::TypesBack(plan_digest(&other)));
    let err = block_on(confirm(&fake, &ada(), &prompt)).expect_err("another digest");
    assert!(matches!(err, Stop::Refused(Refusal::ConfirmDeclined)));
}

#[test]
fn no_and_unavailable_are_refusals_not_confirmations() {
    let prompt = prompt_for(&Publishing::of(b"bytes"));
    let fake = FakeCorpus::new();

    fake.set_reply(Reply::Always(Confirmed::No));
    assert!(matches!(
        block_on(confirm(&fake, &ada(), &prompt)),
        Err(Stop::Refused(Refusal::ConfirmDeclined))
    ));

    fake.set_reply(Reply::Always(Confirmed::Unavailable));
    assert!(matches!(
        block_on(confirm(&fake, &ada(), &prompt)),
        Err(Stop::Refused(Refusal::NeedsHumanConfirm))
    ));
}

#[test]
fn an_agent_is_refused_before_anybody_is_asked() {
    let prompt = prompt_for(&Publishing::of(b"bytes"));
    let fake = FakeCorpus::new();
    fake.set_reply(Reply::Always(Confirmed::Yes));

    let err = block_on(confirm(&fake, &agent("ada", "some-tool"), &prompt))
        .expect_err("an agent cannot confirm");
    assert!(matches!(err, Stop::Refused(Refusal::AgentCannotConfirm)));
    assert!(fake.asked().is_empty());
}

#[test]
fn a_declared_human_and_an_undeclared_judge_are_both_asked() {
    let prompt = prompt_for(&Publishing::of(b"bytes"));
    let fake = FakeCorpus::new();
    fake.set_reply(Reply::Always(Confirmed::Unavailable));

    // The declaration is not the answer: with nobody to ask, neither goes through.
    for judge in [ada(), undeclared()] {
        assert!(matches!(
            block_on(confirm(&fake, &judge, &prompt)),
            Err(Stop::Refused(Refusal::NeedsHumanConfirm))
        ));
    }
    assert_eq!(fake.asked().len(), 2);
}

#[test]
fn a_confirmation_holds_only_for_the_plan_it_was_given_for() {
    let publishing = Publishing::of(b"bytes");
    let fake = FakeCorpus::new();
    fake.set_reply(Reply::Always(Confirmed::Yes));
    let confirmation =
        block_on(confirm(&fake, &ada(), &prompt_for(&publishing))).expect("confirmed");

    assert_eq!(confirmation.holds_for(&plan_digest(&publishing)), Ok(()));

    // The file changed after the question: the plan made again is another plan.
    let changed = Publishing::of(b"edited in between");
    assert_eq!(
        confirmation.holds_for(&plan_digest(&changed)),
        Err(Refusal::ConfirmationStale {
            confirmed: plan_digest(&publishing),
            now: plan_digest(&changed),
        })
    );
}

#[test]
fn a_confirmation_of_one_kind_of_thing_does_not_hold_for_another() {
    // The same digest under another kind is not what the person was asked about.
    let publishing = Publishing::of(b"bytes");
    let fake = FakeCorpus::new();
    fake.set_reply(Reply::Always(Confirmed::Yes));
    let confirmation =
        block_on(confirm(&fake, &ada(), &prompt_for(&publishing))).expect("confirmed");

    let abort = crate::app::commit::fixtures::Aborting::of(b"bytes");
    let adoption = crate::app::commit::fixtures::adoptable_finding()
        .confirmation()
        .expect("an adoption always asks");
    for other in [ConfirmDigest::Abort(abort.plan.digest()), adoption] {
        assert_eq!(
            confirmation.holds_for(&other),
            Err(Refusal::ConfirmationStale {
                confirmed: plan_digest(&publishing),
                now: other,
            })
        );
    }
}

#[test]
fn a_failing_confirm_port_is_a_port_failure() {
    let prompt = prompt_for(&Publishing::of(b"bytes"));
    let fake = FakeCorpus::new();
    fake.fail(PortName::Confirm);
    assert!(matches!(
        block_on(confirm(&fake, &ada(), &prompt)),
        Err(Stop::Port(AppError::Port {
            port: PortName::Confirm,
            ..
        }))
    ));
}
