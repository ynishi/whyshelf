---
id: 01a1c3d2-fa37-74bf-bbd8-1dbc0e039102
key: DEC-6
title: "An approval fixes the files it stands on as artifacts"
serves: ["REQ-2", "REQ-3"]
related: ["DEC-3"]
rejected: ["Named tags in the code and a state for each tag: more states and a second notation for what a file's hash already says", "Fix only the revision of the code: every commit would make every approval stale"]
revisit: "When hashing a whole file is too noisy: hash the test inside it instead (the adapter's choice)"
rustdoc: ["whyshelf_core::domain::shelf::artifact", "whyshelf_core::domain::shelf::freshness::Cause"]
---
A file of the project is one more kind of upstream. A depends link to it pins its hash (or that it is absent) in the approval, and freshness reads it with the two states a document has: changed is stale, gone is broken. A path with no file yet is an absent artifact, not a broken link, since tests may come after the spec.
