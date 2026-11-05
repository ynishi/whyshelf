use super::*;
use crate::app::commit::fixtures::{Publishing, ada};
use crate::app::fake::{FakeCorpus, Reply, block_on};
use crate::app::ports::{ConfirmAction, PlanSummary};
use crate::domain::judge::test_support::agent;
use crate::domain::judge::{PersonName, Principal};

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
    assert_eq!(prompt.plan(), publishing.plan.digest());
    assert_eq!(
        prompt.summary(),
        PlanSummary::new(ConfirmAction::Publish, publishing.plan.records().len())
    );
    assert_eq!(prompt.plan().short().len(), 8);
}

#[test]
fn a_yes_to_a_prompt_is_a_confirmation_of_that_plan() {
    let publishing = Publishing::of(b"bytes");
    let prompt = prompt_for(&publishing);
    let fake = FakeCorpus::new();
    fake.set_reply(Reply::Always(Confirmed::Yes));

    let confirmation = block_on(confirm(&fake, &ada(), &prompt)).expect("confirmed");
    assert_eq!(confirmation.plan(), publishing.plan.digest());
    assert_eq!(fake.asked(), vec![prompt]);
}

#[test]
fn a_person_who_types_back_the_digest_confirms_and_one_who_types_another_does_not() {
    let publishing = Publishing::of(b"bytes");
    let prompt = prompt_for(&publishing);
    let fake = FakeCorpus::new();

    fake.set_reply(Reply::TypesBack(publishing.plan.digest()));
    assert!(block_on(confirm(&fake, &ada(), &prompt)).is_ok());

    let other = Publishing::of(b"other bytes");
    assert_ne!(other.plan.digest(), publishing.plan.digest());
    fake.set_reply(Reply::TypesBack(other.plan.digest()));
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

    assert_eq!(confirmation.holds_for(publishing.plan.digest()), Ok(()));
    assert_eq!(confirmation.holds_for_plan(&publishing.plan), Ok(()));

    // The file changed after the question: the plan made again is another plan.
    let changed = Publishing::of(b"edited in between");
    assert_eq!(
        confirmation.holds_for_plan(&changed.plan),
        Err(Refusal::ConfirmationStale {
            confirmed: publishing.plan.digest(),
            now: changed.plan.digest(),
        })
    );
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
