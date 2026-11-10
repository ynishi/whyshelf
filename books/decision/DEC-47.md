---
id: "01a1c64b-b4b9-723c-b0d7-3a6afd9617d3"
key: "DEC-47"
issues: ["#39"]
rejected: ["Keep a table of the books in CONTRIBUTING: a copy that nothing ties to the declaration", "Answer the drawing in text alone: one command would answer outside the versioned JSON"]
related: ["DEC-31"]
rustdoc: ["whyshelf_core::domain::shelf::map::ShelfMap", "whyshelf_core::app::shelf::MapReport", "whyshelf"]
serves: ["REQ-16", "REQ-4"]
title: "map answers the checked declaration, in the order written, with a drawing in one field"
---
whyshelf map reads the declaration only. When it can be used, the answer holds every book and every link in the order the declaration writes them, and the drawing of them as one text field; when it cannot, the answer holds its problems, as check's does, with the same exit status. One pure function of the checked schema gathers what the map says.
