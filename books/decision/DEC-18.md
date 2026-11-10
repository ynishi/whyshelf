---
id: "01a1c4ba-1413-758c-a10a-55cdcaa23432"
key: "DEC-18"
rejected: ["Trace the frames of approvals: trace answers what is linked now and freshness answers what an approval saw"]
related: ["DEC-15"]
rustdoc: ["whyshelf_core::domain::shelf::trace::trace"]
serves: ["REQ-3"]
title: "Trace walks the links the documents hold now, both ways, each document once"
---
Trace follows every kind of link from a document, upstream and downstream, transitively, and expands each document once. A link that cannot be followed and a link to a file are edges that end there.
