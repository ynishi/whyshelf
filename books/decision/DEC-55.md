---
id: "01a1c847-f192-7237-ad26-fff5f77b2194"
key: "DEC-55"
issues: ["#58"]
rejected: ["Record stats as each command runs: a reading command would write, and the numbers would depend on who ran what", "Measure how long commands take in the same answer: time per run is a question about speed, kept for its own change", "Count an origin by any pin that differs, links included: a link edited would read as an artifact or a document that changed"]
related: ["DEC-35", "DEC-27"]
rustdoc: ["whyshelf_core::domain::shelf::stats", "whyshelf_core::domain::id::DocId::made_at_millis", "whyshelf_core::app::shelf::ShelfApp::judge_all", "whyshelf"]
serves: ["REQ-21", "REQ-5", "REQ-2"]
title: "stats is one pure function of the ledger and the shelf, with each measure defined once"
---
whyshelf stats reads the ledger and the documents and writes nothing. One pure function of the two gives four measures, each defined once on it: approvals per document and book, those made after an earlier approval of the document that is not revoked, and the revoked apart; the time from a document's identity, a UUID v7 that carries when it was made, to its first approval that is not revoked, per book, by nearest rank; runs, the decisions given one time by one actor, as every run of judge gives every decision of it the time of its reading (DEC-27); and origins, the pins held by an approval made again and by the latest earlier one that is not revoked, of the same comparison, whose hash or state differs. No measure reads a clock: since keeps only what was judged at or after it. The same ledger and documents give the same answer, order included.
