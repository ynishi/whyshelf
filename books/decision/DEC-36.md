---
id: "01a1c55d-ea98-7060-acb2-b3c19c484a89"
key: "DEC-36"
issues: ["#30"]
rejected: ["Keep one of the holders and drop the others: which one is kept would hang on the order the files are read"]
related: ["DEC-12"]
rustdoc: ["whyshelf_core::domain::shelf::build::DocProblem"]
serves: ["REQ-8"]
title: "Two holders of one identity or of one key are all invalid"
---
Two files with one identity, or one key twice in a book, make every holder invalid, each with the problem that names the others.
