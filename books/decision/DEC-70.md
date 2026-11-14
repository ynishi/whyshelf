---
id: "01a1da61-c4da-7651-9e22-b50a80e5726b"
key: "DEC-70"
issues: ["#101"]
rejected: ["Filter the rows in the command line: the rule of what passes and the check against the declaration would sit in a surface", "A query language: a grammar and its manual for what three kinds of condition already say", "Fold unlinked --book into the same filter now: unlinked answers lists per book, another shape, and can be moved onto it later"]
related: ["DEC-45"]
revisit: "When another command needs the same filter, or a condition other than equality is asked for"
rustdoc: ["whyshelf_core::domain::shelf::filter::StatusFilter", "whyshelf_core::domain::shelf::filter::StatusFilterAsk", "whyshelf_core::domain::shelf::filter::FilterProblem", "whyshelf_core::domain::shelf::doc::FieldValue::read", "whyshelf"]
serves: ["REQ-30"]
title: "A status filter is checked against the declaration in the domain, and admits a document by its file's book, its standing and its field values"
---
StatusFilter::new checks what a person asked against the declaration and returns every problem: a book not declared, a field no book declares, a value no declaring book can read as its type. Conditions of one kind are alternatives and kinds combine; each field condition must hold. A document's book is the book its file is under, for a valid document and an invalid file alike, the rule the tally by book uses; a document with no file has no book and no fields. A field value is compared as its type reads it, through the one reading of a text as a field value that new and the front matter use too; a list holds the text as one item. status keeps its answer and status_filtered adds the filter beside the rows; health stays the whole project's. The command line only reads the shape of each argument, and reads a standing by the words an answer prints.
