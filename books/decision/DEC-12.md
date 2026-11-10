---
id: "01a1c4ba-1228-72b0-9fcb-2a2df7330d46"
key: "DEC-12"
rejected: ["Stop at the first broken file: one bad file would hide the state of every other", "Call an identity gone while some file's identity cannot be read: it may be in that file"]
related: ["DEC-4"]
rustdoc: ["whyshelf_core::domain::shelf::build::Shelf::build", "whyshelf_core::domain::shelf::build::DocProblem", "whyshelf_core::domain::shelf::build::DocState", "whyshelf_core::domain::shelf::doc::UnresolvedWhy"]
serves: ["REQ-3", "REQ-8"]
title: "The build never fails; a file it cannot trust is an invalid document with every problem"
---
Every file becomes a valid document or an invalid one carrying all its problems; a link that cannot be followed is a state of a valid document. A file whose front matter cannot be parsed keeps its identity when its id line can be read.
