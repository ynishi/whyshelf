---
id: "01a1cae7-17e9-7631-bef0-e6d4c9095984"
key: "DEC-65"
issues: ["#78"]
rejected: ["Accept an exemption that is unneeded at once: it adds nothing to the ledger, as an approval that repeats does not (DEC-58)", "Write an exemption as a field of the document: a document cannot speak for a person's decision about it (REQ-9)"]
related: ["DEC-23", "DEC-50", "DEC-9", "DEC-59"]
rustdoc: ["whyshelf_core::domain::shelf::exemption::exemption_reading", "whyshelf_core::domain::shelf::exemption::frame_exempt", "whyshelf_core::domain::shelf::exemption::cover_of", "whyshelf_core::domain::shelf::exemption::Unneeded", "whyshelf_core::domain::shelf::cover::covering_reach", "whyshelf_core::domain::shelf::cover::counts", "whyshelf_core::app::shelf::ShelfApp::exempt"]
serves: ["REQ-28", "REQ-12", "REQ-9"]
title: "judge exempt records an exemption of one document from one covering link, read by one rule beside the cover"
---
whyshelf judge exempt <ref>... --from <book>.<link> --reason writes a record of kind exempt: the document, the link, an actor, a time and a reason. One rule, exemption_reading, reads an exemption both when it is asked for and on every check, and reach is counted once, by the walk the cover itself uses. An exemption is needed while the link covers the document's book, the document is valid and not decided against, and the link does not reach it; otherwise it is unneeded, why told as reached, not_declared, no_cover, other_book, not_valid or decided_against. One unneeded at once is refused, as is a document not valid, a link with no cover or into another book, and a second of one document from one link, in the run or before it; the run is decided whole. A needed exemption takes the place of not_covered, so the document leaves uncovered and an error cover no longer fails check; check lists the exemptions, those unneeded and any held twice, the last two also on stderr with the command to withdraw one. judge withdraw takes an exemption back.
