---
id: "01a1c55d-f3b7-72cf-8b09-aa059e4584fe"
key: "DEC-39"
issues: ["#30"]
rejected: ["Make every approval expire after a fixed time: most decisions hold until something they stand on changes"]
related: ["DEC-15", "DEC-14"]
rustdoc: ["whyshelf", "whyshelf_core::domain::shelf::freshness::Cause", "whyshelf_core::domain::shelf::freshness::Notice"]
serves: ["REQ-2"]
title: "An approval may expire, and an expired approval is the weakest degradation"
---
An approval's record may keep a time after which it no longer holds. From that time the approval is expired, the weakest of the three strengths; how far that travels is DEC-15's rule. judge approve --until gives the time.
