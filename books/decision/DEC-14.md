---
id: "01a1c4ba-12cd-738a-8346-061089dc647a"
key: "DEC-14"
rejected: ["Check a decision after it is written: a refused judgement would leave a record behind"]
related: ["DEC-5"]
rustdoc: ["whyshelf_core::domain::shelf::ledger::frame_approve", "whyshelf_core::domain::shelf::ledger::frame_present", "whyshelf_core::domain::shelf::ledger::frame_retire", "whyshelf_core::domain::shelf::ledger::FrameRefusal", "whyshelf_core::domain::shelf::ledger::DecisionProblem", "whyshelf_core::domain::shelf::ledger::JudgedAt"]
serves: ["REQ-2"]
title: "A decision is made only from its frame, and its invariants are checked before any record exists"
---
An approval is made from the facts the shelf supplies: the document must be valid, every depends link must be followed, and it must not close a cycle of links or of approvals; what refuses an approval because of what was decided about its upstream is DEC-5's rule. A decision with an expiry that is not after it, or a frame that names the document itself, is refused. A retirement needs no file. Time is a number of milliseconds passed in by the caller.
