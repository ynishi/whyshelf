---
id: "01a1c3eb-4cc2-7179-8acb-f6442ce9a900"
key: "DEC-8"
issues: ["#16"]
rejected: ["Let people make ids and keys by hand: the next number and a UUID v7 are easy to get wrong", "Write through a template file: a second place for the rules of a book"]
related: ["DEC-1", "DEC-28"]
rustdoc: ["whyshelf_core::domain::shelf::build::Shelf::check_new", "whyshelf_core::app::shelf::ports::DocWrite"]
serves: ["REQ-5", "REQ-1"]
title: "Documents are created by a command that never overwrites"
---
`whyshelf new` takes a key by DEC-28's rule, a fresh identity, and the entries; checks the document with the rules a build uses before anything is written; and creates one new file, never over an existing one. The command assumes no field of a book: a title is an entry like any other, and `--title` is only a shorthand for it, so a book with no title field can have documents made, and whether a title is required is the book's declaration.
