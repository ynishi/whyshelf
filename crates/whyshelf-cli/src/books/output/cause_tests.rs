//! Tests of the words of an exemption that is not needed because a decision went
//! against its document. The words of an upstream that does not stand as a
//! settled approval are tested beside `upstream_status`, in `cause.rs`.

use whyshelf_core::domain::shelf::exemption::Unneeded;
use whyshelf_core::domain::shelf::ledger::NotStanding;

use super::cause::unneeded_word;

#[test]
fn an_exemption_of_a_document_decided_against_is_told_the_same_whichever_decision() {
    for status in [
        NotStanding::Denied,
        NotStanding::Deprecated,
        NotStanding::Retired,
    ] {
        assert_eq!(
            unneeded_word(&Unneeded::DecidedAgainst(status)),
            "decided_against"
        );
    }
}
