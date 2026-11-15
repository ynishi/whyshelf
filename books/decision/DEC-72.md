---
id: "01a1df93-423e-76d2-9471-d810f8e8c543"
key: "DEC-72"
issues: ["#123"]
rejected: ["Forbid a blank text in every text field, required or not, as a type: it makes projects that write an optional text empty invalid, and changes what a filter on an empty value means", "Call a blank required field missing: it is written, and a person told it is missing looks for an entry that is there", "Refuse a blank title only in new: check would go on passing a file written by hand, and the rule would sit in two places"]
related: ["DEC-8", "DEC-12"]
revisit: "When an optional field written empty, a blank item of a list, or a line break in a text needs a meaning"
rustdoc: ["whyshelf_core::domain::shelf::schema::FieldPresence", "whyshelf_core::domain::shelf::build::DocProblem"]
serves: ["REQ-8"]
title: "A required field says something: a text that is empty or blank, or a list with no item, is a problem of the document"
---
One judgement says whether a declared field meets its declaration, given what a document wrote or that it wrote nothing: its type, and for a required field that the value says something. A required text says something when it is not empty or white space only, a required list when it has at least one item; an enum, a whole number or a truth value written is a value. A required field left out is missing; one written with nothing in it is empty or blank, a problem of the document beside a value of the wrong kind. new and check build documents the same way, so new refuses what check would call invalid. An optional field keeps taking an empty value.
