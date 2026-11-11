---
id: "01a1c3d2-fb7c-725e-8704-96f8a67f82a2"
key: "DEC-7"
rejected: ["Keep a copy of every approved text beside the ledger: the same content in two places", "Derive the approval's point from the history alone: squash and rebase move it"]
related: ["DEC-6", "DEC-1"]
revisit: "When content that was approved and never committed must still be read back"
rustdoc: ["whyshelf_core::domain::shelf::snapshot", "whyshelf_core::domain::shelf::snapshot::SnapshotItem::retrieval", "whyshelf_core::domain::shelf::snapshot::SnapshotItem::changed_from", "whyshelf_core::domain::shelf::artifact::ContentAddress", "whyshelf_core::domain::shelf::artifact::ContentAddress::from_validated", "whyshelf_core::app::shelf::ports::ContentStore", "whyshelf_infra::shelf::git"]
serves: ["REQ-7", "REQ-11"]
title: "The verdict is the hash; the content is read back from version control"
---
Each approval keeps, beside each hash, an opaque address the version-control adapter can read the content back from, and the revision it was made at. An address names the file as it was in the working tree when the approval was made: the blob its content is, or would be, in Git, committed or not, under Git's own conversions such as line endings, with every filter driver the repository's configuration names shut out, and no address at all when a driver's name cannot be written exactly to shut it out; a file a filter program would convert is stored as another blob and cannot be read back. The domain never decides freshness with them. An approved text whose blob Git does not have cannot be read back, and the answer says so. An address is checked by the adapter, and again before Git is asked for it, so a record cannot pass an option to Git. Putting content back in place is Git's work and a person's, never this program's.
