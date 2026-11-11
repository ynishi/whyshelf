---
id: "01a1c690-6834-77b2-8c4a-bd554e7eb233"
key: "DEC-50"
issues: ["#49", "#43"]
rejected: ["Keep cover a gate only: a book whose documents are expected to be reached but may wait would have no expectation at all", "Exempt documents one by one instead: an exemption is for a document, and the strength is for what a project expects of a whole link"]
related: ["DEC-23", "DEC-49"]
rustdoc: ["whyshelf_core::domain::shelf::schema::CoverSeverity", "whyshelf_core::app::shelf::BooksCheck::is_clean", "whyshelf"]
serves: ["REQ-12"]
title: "A covering link declares a strength: error fails the check, warn lists and leaves it clean"
---
cover takes true or error, warn, or false. A document a covering link does not reach has the not_covered notice and is listed in check's uncovered with the strength; only an error gap makes check not clean, and a document exempted from the link is no gap (DEC-65). Each warn gap is also one line on stderr, so a run that shows the answer only when it fails still shows it; stdout stays the JSON answer alone, which is no text answer beside it (DEC-31). Every answer that shows a cover expectation shows its strength, and map keeps cover true for both.
