---
id: "01a1c6a9-7880-7658-9fdc-915180d46b03"
key: "DEC-51"
issues: ["#41"]
rejected: ["Approve the broken too: an approval would pin as gone an artifact it saw, or stand on an invalid upstream", "Fail the whole run when one would be refused: one deprecated upstream would keep every other stale approval from being approved again", "Order by the books' declaration: a book can link into its own book, and the order would be this repository's", "Take references beside --stale: one run would mean two things", "Narrow the run to what one origin reaches (--origin): not needed while a change's stale approvals are all looked at again together"]
related: ["DEC-27", "DEC-15", "DEC-39"]
revisit: "When a change must approve again only what one origin reaches, and not the other stale approvals"
rustdoc: ["whyshelf_core::domain::shelf::order::upstream_first", "whyshelf_core::app::shelf::ShelfApp::approve_stale", "whyshelf", "whyshelf_core::domain::shelf::order::held_back"]
serves: ["REQ-18", "REQ-5"]
title: "judge approve --stale approves again, in one run upstream first, what would hold, and names what it leaves out"
---
judge approve --stale takes no reference and a reason. It approves again the approvals that are expired or stale and would hold once approved again; every other approval that does not hold is left out with a reason, broken (it is broken now), refused (the model would refuse the approval now, with the refusal's text) or stands_on (a current depends link names a document that is broken, pending, or itself left out in the run; a pending one is approved first with judge approve), and each is told on stderr. The candidates are taken upstream first (upstream_first) on one reading, so one pass sees every upstream decided or left out first, and each is decided on the ledger the earlier ones of the run left; everything decided is written together, and only a failed port or a broken invariant fails the run, writing nothing. Pending documents are not touched. One --until applies to every approval of the run, an earlier expiry is not carried over, and one not after now is refused before anything is selected. A name with nothing behind it is pinned as absent and told, as by any approval; what is never approved again is an artifact pinned present and gone.
