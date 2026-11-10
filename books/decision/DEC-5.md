---
id: "01a1c333-d435-7f4d-abb6-036e89451d8b"
key: "DEC-5"
rejected: ["Refuse every approval that would be broken from the start: needs freshness inside the ledger and turns the layers around", "Refuse nothing and let freshness say it: an approval of a denied upstream is noise from the start"]
related: ["DEC-3"]
revisit: "When gates are added: a gate may refuse on how the upstream stands now"
rustdoc: ["whyshelf_core::domain::shelf::ledger::frame_approve", "whyshelf_core::domain::shelf::ledger::NotStanding"]
serves: ["REQ-2", "REQ-3"]
title: "An approval cannot stand on an upstream that people decided does not stand"
---
What a decision already says about an upstream (denied, deprecated, retired) refuses an approval that pins it; a deprecated upstream is to be replaced, so the person links to its successor instead and approves on that. How the upstream stands now (only pending, or approved and itself degraded) does not refuse, since it can change without a decision about this document; freshness says it.
