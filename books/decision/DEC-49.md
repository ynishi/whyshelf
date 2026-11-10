---
id: "01a1c671-1d95-725c-bd74-8b605650d16c"
key: "DEC-49"
issues: ["#47"]
rejected: ["Use cover as the only way to find them: it is a rule that fails the check, and some books are not expected to be covered", "List every document with no incoming or no outgoing link: the books no link can reach, or that declare no link, fill the lists and bury the isolated ones", "Count a link from a document decided against as reaching: a document decided against would hide a live one, which cover refuses too", "Call it orphans: other traceability tools use the word for a broken link"]
related: ["DEC-23", "DEC-18"]
rustdoc: ["whyshelf_core::domain::shelf::trace::Linkage", "whyshelf_core::domain::shelf::trace::linkage", "whyshelf_core::app::shelf::UnlinkedReport", "whyshelf"]
serves: ["REQ-17", "REQ-12"]
title: "unlinked answers the isolated documents, then those with no link in or no link out where the declaration lets one be"
---
whyshelf unlinked reads the links the documents hold now, of every meaning, whatever cover says. A link reaches its target only when its holder stands (pending or approved, as for cover, DEC-23) and the target is another valid document; a link to itself, a key that cannot be followed and a link to artifacts reach nothing, and any link a document holds counts as holding one. What the declaration makes inevitable is left out, judged from the schema alone: every declared link counts except replaces, since a successor's link is absent from every original by nature: a book is reachable when such a link, of any book including its own, points into it, and may hold links when it declares one, to artifacts or into any book including its own; a book that is neither is cut off and none of its documents is listed. The three lists are disjoint: isolated, reached by none and holding none, in a book reachable or that may hold; no_incoming, reached by none but holding one, in a reachable book; no_outgoing, reached but holding none, in a book that may hold. Documents decided against are listed in the same three lists apart. The exit is 0 whenever the declaration can be used; an unusable one exits 1, as map does.
