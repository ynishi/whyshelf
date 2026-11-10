---
id: "01a1c4d3-a3a0-7506-8312-b5aef92e4742"
key: "DEC-23"
issues: ["#13"]
rejected: ["Name the expected books in the core: the core knows no kind of document", "Declare the expectation on the target book by naming a book and link: a name could point at nothing and would need its own check", "Make a document no expected link reaches stale: its own content did not change and an approval of it still holds", "Count a denied or retired document as reaching: what was decided against meets nothing"]
related: ["DEC-11", "DEC-15"]
revisit: "When an expectation needs more than one document reaching it, or a covering link into its own book must reach the first document of a chain"
rustdoc: ["whyshelf_core::domain::shelf::schema::LinkKind", "whyshelf_core::domain::shelf::freshness::Notice"]
serves: ["REQ-12", "REQ-1"]
title: "A link into a book can declare that it is expected to reach every document of that book"
---
A link into a book may say cover = true: every pending or approved document of the target book is expected to be reached by that link from a pending or approved document. One that is not is told as a notice on it and listed by check, which is then not clean; its freshness is unchanged. A link to files cannot say it.
