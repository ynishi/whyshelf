---
id: "01a1e20f-84aa-7350-905c-6c3af70e7219"
key: "DEC-74"
issues: ["#124"]
rejected: ["A lock over the book's directory from choosing the key to writing it: it keeps out only other runs of whyshelf, not an editor or Git, so the create-only write and a second choice are needed anyway, and a lock left by a run that died stops the next one; numbers in the order runs started are not asked for", "Read the shelf again when the file is there: reading costs time in the number of documents, and many runs at once would read it many times", "Only tell a chosen key that was taken apart from a key asked for that was taken, and let the caller run again: runs started together would still fail"]
related: ["DEC-28"]
revisit: "When numbers must follow the order runs started, or several documents must be written as one"
rustdoc: ["whyshelf_core::domain::shelf::build::Shelf::replan_after_taken", "whyshelf_core::domain::shelf::build::NewKey", "whyshelf_core::domain::shelf::build::NoFreeKey", "whyshelf_core::app::shelf::ShelfApp::new_doc"]
serves: ["REQ-5"]
title: "A chosen key whose file is there when new writes is passed over for the next number, up to a bound; a key asked for is still refused"
---
A new key keeps whether it was chosen as the next number or asked for. When the create-only write finds a file at its path, the domain decides what follows: a chosen key gives the next number, made by the same grammar and range as the next key, and the document is judged again at it, with the same identity and the same answers of the adapters; a key asked for is refused as before. After 64 keys each with a file, new stops, writes nothing, and says that other runs may be writing at the same time or that files that are no valid document are in the way, to be run again once they are done or put right. The shelf is not read again, and nothing locks: the create-only write is the one point where a key is taken.
