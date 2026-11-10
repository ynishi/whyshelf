---
id: 01a1c3d2-fb7c-725e-8704-96f8a67f82a2
key: DEC-7
title: "The verdict is the hash; the content is read back from version control"
serves: ["REQ-7"]
related: ["DEC-6", "DEC-1"]
rejected: ["Keep a copy of every approved text beside the ledger: the same content in two places", "Derive the approval's point from the history alone: squash and rebase move it"]
revisit: "When content that was approved and never committed must still be read back"
rustdoc: ["whyshelf_core::domain::shelf::snapshot", "whyshelf_core::domain::shelf::artifact::ContentAddress"]
---
Each approval keeps, beside each hash, an opaque address the version-control adapter can read the content back from, and the revision it was made at. The domain never decides freshness with them. An approved text that was never committed cannot be read back, and the answer says so.
