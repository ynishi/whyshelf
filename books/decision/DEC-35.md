---
id: "01a1c55d-e6b5-775e-9fa6-11ec41ab044a"
key: "DEC-35"
issues: ["#30"]
rejected: ["Use the path or the key as the identity: a rename or a new key would make it another document"]
replaces: ["DEC-21"]
rustdoc: ["whyshelf_infra::shelf::system::UuidV7Ids", "whyshelf_core::domain::id::EntityId"]
serves: ["REQ-9", "REQ-10"]
title: "Identities are UUID v7, made outside the domain"
---
Every identity of a document or a record is a UUID v7. The domain makes none (it has no clock and no randomness): an adapter makes it and the caller passes it in. Any other identity is refused when it is read.
