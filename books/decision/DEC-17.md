---
id: "01a1c4ba-13c1-71ff-8447-3222d6d270df"
key: "DEC-17"
issues: ["#117"]
rejected: ["Match a short identity against the start of an identity: identities made together share it", "Choose among several hits: the person names one"]
rustdoc: ["whyshelf_core::domain::shelf::lookup::Reference", "whyshelf_core::domain::shelf::lookup::resolve", "whyshelf_core::domain::shelf::lookup::Resolution", "whyshelf_core::domain::id::ShortId"]
serves: ["REQ-6"]
title: "References are read by one reader with four shapes in a fixed order, and several hits are listed"
---
A reference is a key in a book, an identity, the end of an identity, or a bare key, tried in that order by one reader before anything runs. An unknown book, a key the grammar refuses, a key no document has, and a key that only files with no identity that reads write (Resolution::NoIdentity, which names the files; who writes a key is read as DEC-94 says) are four different answers. A document whose file is gone can still be named through the ledger.
