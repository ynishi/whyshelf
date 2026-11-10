---
id: "01a1c332-3ad8-7753-b9e1-a7cd0b06ed77"
key: "DEC-1"
rejected: ["JSON Lines: two branches append at the same end and conflict", "SQLite: cannot be merged by git"]
revisit: "When the number of records nears what a file system handles well"
rustdoc: ["whyshelf_infra::shelf::ledger"]
serves: ["REQ-2"]
title: "One record per file, in JSON"
---
Each decision is a file named by its own identity, so two branches that judge add two files and merge without a conflict.
