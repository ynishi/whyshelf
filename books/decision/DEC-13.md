---
id: "01a1c4ba-127a-726b-bdcc-3cb3ce7e5157"
key: "DEC-13"
rejected: ["Believe one of two different records with one identity: neither has a reason to be believed over the other", "Skip a ledger file that is no record: a fault would vanish silently"]
related: ["DEC-1", "DEC-2", "DEC-88"]
rustdoc: ["whyshelf_core::domain::shelf::ledger::Ledger::build", "whyshelf_core::domain::shelf::ledger::Ledger::status_of", "whyshelf_core::domain::shelf::ledger::Ledger::check_withdraw", "whyshelf_core::domain::shelf::ledger::LedgerProblem", "whyshelf_core::domain::shelf::ledger::Record::from_canon"]
serves: ["REQ-2", "REQ-8"]
title: "The latest decision that is not revoked stands, and records that contradict one another are set aside"
---
A record is a decision, a declaration that an edit keeps the meaning, or a revocation, and it is read back strictly. The decision that stands about a document is the latest that is not revoked, by time and then by identity, of those the rules of who may decide admit (DEC-88): a record they do not admit is not used, and is set aside as a problem of the ledger until a revocation they admit withdraws it (a revocation they do not admit withdraws nothing and is never withdrawn itself: its problem lasts until a revocation they admit withdraws what it named); in a project that declares no rule, every record is admitted. Two different records with one identity are a conflict and neither is used; a revocation of what is not there, of a revocation or of a conflict withdraws nothing, and nor does one the rules do not admit, for the record it is not admitted to withdraw. A record is withdrawn by revoking the standing decision of a document or by naming the record, a decision or declaration not admitted included.
