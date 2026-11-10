---
id: "01a1c4ba-1504-776c-973e-60e3911490da"
key: "DEC-21"
rejected: ["Name the actor in a committed file: anyone could write a judgement in another person's name"]
rustdoc: ["whyshelf_infra::shelf::system::ProjectActor", "whyshelf_infra::shelf::system::UuidV7Ids", "whyshelf_core::domain::actor::Actor", "whyshelf_core::domain::id::EntityId"]
serves: ["REQ-9"]
title: "The actor is never read from a committed file, and identities are UUID v7"
---
Who judges is the actor given on the command line, else the environment variable WHYSHELF_ACTOR, else Git's user name. Every identity of a document or a record is a UUID v7, and any other is refused when it is read.
