---
id: "01a1c55d-d0dc-701e-8908-afb6fafe946e"
key: "DEC-28"
issues: ["#30"]
rejected: ["Count only the valid documents: the key of a broken file would be handed out again"]
related: ["DEC-8"]
replaces: ["DEC-19"]
rustdoc: ["whyshelf_core::domain::shelf::build::Shelf::next_key", "whyshelf_core::domain::shelf::build::Shelf::claim_key"]
serves: ["REQ-5"]
title: "A new document takes a key no file of its book writes"
---
The next key of a book is one more than the highest number any file of the book writes as its key, valid or not, and a key given by a person is refused when any file of the book writes it. A file whose front matter cannot be read writes no key that is seen, and is not counted.
