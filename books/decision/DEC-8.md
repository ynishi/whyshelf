---
id: "01a1c3eb-4cc2-7179-8acb-f6442ce9a900"
key: "DEC-8"
rejected: ["Let people make ids and keys by hand: the next number and a UUID v7 are easy to get wrong", "Write through a template file: a second place for the rules of a book"]
related: ["DEC-1"]
rustdoc: ["whyshelf_core::domain::shelf::build::Shelf::next_key", "whyshelf_core::domain::shelf::build::Shelf::check_new"]
serves: ["REQ-5"]
title: "Documents are created by a command that never overwrites"
---
`whyshelf new` takes the next key of the book (or a given one that is not taken), a fresh identity, and the entries; checks the document with the rules a build uses before anything is written; and creates one new file, never over an existing one. Reading commands write nothing.
