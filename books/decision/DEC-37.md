---
id: "01a1c55d-ede0-77df-9261-99dde9095582"
key: "DEC-37"
issues: ["#30"]
rejected: ["Hash the bytes as they are: a change of line endings by an editor or by Git would make approvals stale", "Normalize Unicode too: the rule would hang on a version of Unicode, and texts that differ would share a hash"]
related: ["DEC-12"]
rustdoc: ["whyshelf_core::domain::shelf::doc::normalize"]
serves: ["REQ-10"]
title: "A document's text is normalized by one rule of the domain before it is hashed"
---
Line breaks become LF, the spaces and tabs at the end of each line are removed, and the text ends in exactly one LF, or is empty when nothing else is in it; there is no Unicode normalization. The rule is a function of the domain. The adapter that reads a file applies it to the whole text before the front matter is read, and the hash of a document applies it to the body again, so what is hashed does not hang on the adapter.
