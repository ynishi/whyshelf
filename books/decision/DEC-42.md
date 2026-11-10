---
id: "01a1c5c5-778a-7288-ae20-3289a8a7a718"
key: "DEC-42"
issues: ["#36"]
rejected: ["Make check fail on a path with no file: evidence not written yet is allowed (DEC-6), and a gate on it would refuse it", "Tell it in check alone: a wrong path is pinned at the approval, and the person approving is the one to see it"]
related: ["DEC-6"]
rustdoc: ["whyshelf_core::domain::shelf::doc::Doc::absent_artifacts", "whyshelf_core::domain::shelf::freshness::Notice", "whyshelf_core::app::shelf::Absent", "whyshelf_core::app::shelf::Judged", "whyshelf"]
serves: ["REQ-12"]
title: "A name with nothing behind it is told, and leaves the check clean"
---
Every name a pending or approved document's link to artifacts writes that is absent in its space is a notice of its row, a line of check's absent list (the check stays clean), and, for an approval, a line of the judgement's absent list. A denied, deprecated or retired document's names are not told.
