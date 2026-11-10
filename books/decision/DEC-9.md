---
id: "01a1c434-c205-7538-9b17-4fe0f9900392"
key: "DEC-9"
rejected: ["Guess the meaning of an edit: no tool can and none of the traceability tools tries", "Hash the code without its comments: one parser per language and a second notion of equal", "Trust a new approval of the upstream: the downstream saw the older text", "Cover every hash any approval pinned: it hides an older rewrite no one declared"]
related: ["DEC-6"]
revisit: "When one link needs a declaration for most edits: choose for it what is compared or count only a new approval of the upstream"
rustdoc: ["whyshelf_core::domain::shelf::ledger::SameMeaning", "whyshelf_core::domain::shelf::ledger::frame_same_meaning", "whyshelf_core::domain::shelf::freshness::Notice"]
serves: ["REQ-2"]
title: "An edit people declare keeps the meaning is a notice and not a change"
---
A changed hash is a change. A person who has read an edit may declare that it keeps the meaning: a record of the edit from one earlier hash (the one the document's own approval saw, the only one pinned, or the one named) to the hash now, with a reason. Every approval that pinned that earlier hash holds again, and tells the edit as a notice naming the declaration; an older pin, from before a change no one declared, stays a change. A later edit is a change again, and a declaration is withdrawn like a decision.
