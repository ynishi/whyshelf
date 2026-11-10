---
id: "01a1c5ae-3719-7199-87eb-67281a108cef"
key: "DEC-41"
issues: ["#34"]
rejected: ["Group by root, the cause as it is held: an edited file pinned by several approvals is a different cause in each", "Give a cycle and a changed comparison the document that holds them: one cycle or one edit of the declaration would be read once per document", "Group by what happened (edited, gone, invalid) as well as where: one edit of a document can be seen as its own change and as an upstream's, and would split"]
related: ["DEC-15"]
rustdoc: ["whyshelf_core::domain::shelf::freshness::Origin", "whyshelf_core::domain::shelf::freshness::Cause::origin", "whyshelf_core::domain::shelf::freshness::Assessment::origins", "whyshelf"]
serves: ["REQ-15"]
title: "Stale approvals are grouped by the thing where a root starts"
---
Each root of a degraded approval starts at an origin: the upstream a pin names, the file an artifact pin names, the declaration of the books for a comparison a link now declares otherwise, the cycle for a cycle, else the document that holds it (its own text, links or expiry). A Via starts nowhere of its own. stale answers, beside its entries, each origin with the approvals one of whose roots starts there, the origin that degrades the most first; an approval with roots at two origins is under both, and every degraded approval is under one.
