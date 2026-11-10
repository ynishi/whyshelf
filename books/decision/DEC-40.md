---
id: "01a1c577-33e1-735e-b992-19f40a27438a"
key: "DEC-40"
issues: ["#32"]
rejected: ["Show the reason in get alone: status and stale are where a person asks why an approval stood", "Put the whole record in the row: its frame and pins already have their place in get --approved"]
related: ["DEC-31"]
rustdoc: ["whyshelf_core::app::shelf::Row", "whyshelf_core::domain::shelf::ledger::Ledger::standing_decision", "whyshelf_core::domain::shelf::ledger::Decision::reason", "whyshelf"]
serves: ["REQ-14"]
title: "Every row carries the decision that stands, from its record"
---
A row carries the decision that stands about its document, the one its declared state is read from, with the record's id, actor, time and reason; a pending document carries none. The decision is the ledger's standing decision, so the row and the declared state cannot name two different decisions.
