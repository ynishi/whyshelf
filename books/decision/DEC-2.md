---
id: "01a1c332-3b83-7c08-aeeb-5c338057481e"
key: "DEC-2"
related: ["DEC-1"]
rustdoc: ["whyshelf_core::domain::shelf::ledger::Record::hash", "whyshelf_core::domain::canon"]
serves: ["REQ-2"]
title: "The hash is taken over the canonical form, not the file"
---
A record is written from its canonical form and read back into it, so key order, spaces and escapes of the JSON change no hash.
