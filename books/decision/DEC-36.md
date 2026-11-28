---
id: "01a1c55d-ea98-7060-acb2-b3c19c484a89"
key: "DEC-36"
issues: ["#30", "#117"]
rejected: ["Keep one of the holders and drop the others: which one is kept would hang on the order the files are read", "Propose renumbering the document made later, which its UUID v7 tells: the later one is not always the one on the branch that has not merged, and the proposal can point at the documents of the main branch"]
related: ["DEC-12"]
rustdoc: ["whyshelf_core::domain::shelf::build::DocProblem", "whyshelf_core::domain::shelf::doc::KeyCollision", "whyshelf_core::domain::shelf::build::Shelf::collisions"]
serves: ["REQ-8"]
title: "Two holders of one identity or of one key are all invalid"
---
Two files with one identity, or two files of a book that write one key, make every holder invalid, each with the problem that names the other files. A file whose key reads and whose identity does not is a writer of the key, and so a holder (who writes a key is read once, DEC-94). A file that carries an identity a side of a merge conflict in another file shows, and that other file, may hold it twice, and are all invalid the same way, each with the problem that names the other files (DEC-115). check lists each key two or more files write once, in collisions, with every file and the identity of each that could be read; which of them is to take another key it does not say, as that is for a person to decide.
