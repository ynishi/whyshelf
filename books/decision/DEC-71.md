---
id: "01a1db32-2c6f-7019-be31-9436bee5c856"
key: "DEC-71"
issues: ["#102"]
rejected: ["Rebuild the commands in the command line from the causes of the entries: what happened at an origin and what settles it would be decided again in a surface", "Decide in the app: what settles an origin is a rule of the model, and the app only carries answers", "A command of its own that settles one origin: it needs a grammar to name an origin, and the issue asks for the answer of stale", "Offer approving again by the strength of the origin's own causes: judge approve --stale decides per approval over all its causes, so it could be offered where it takes nothing"]
related: ["DEC-26", "DEC-45"]
revisit: "When a settlement proves to be refused often in practice, or another surface wants the same list"
rustdoc: ["whyshelf_core::domain::shelf::settle::settlements", "whyshelf_core::domain::shelf::settle::Settlement", "whyshelf_core::domain::shelf::freshness::OriginEvent", "whyshelf_core::domain::shelf::freshness::Cause::located", "whyshelf_core::domain::shelf::freshness::OriginGroup", "whyshelf"]
serves: ["REQ-31"]
title: "An origin keeps what happened there and who holds the cause, and the domain lists what settles it; the command line writes it from the clap definition"
---
One match over the kinds of cause says where each starts and what happened there; an origin keeps the events of its causes and the approvals that hold a cause there themselves. settlements lists what can settle an origin: read the diff of an approval that holds the cause, declare the edit the same, look for moves and declare one, approve a pending upstream, approve again with an expiry, and approve again what an edit made stale, the last only for an edit, an arrival or a changed link, and only when the shelf's own choice of what --stale takes holds one of the origin's approvals. An upstream that is pending or superseded, a link behind a move, a gone file, an invalid document and a cycle are not offered approving again, since it would settle nothing. The command line writes each as a command, its names drawn from the clap definition and every command checked to parse, documents by their key in their book, with the run's root.
