---
id: "01a1c55d-e310-703a-82cc-fecc69ebdc07"
key: "DEC-34"
issues: ["#30"]
rejected: ["Name the actor in a committed file: anyone could write a judgement in another person's name"]
replaces: ["DEC-21"]
rustdoc: ["whyshelf_infra::shelf::system::ProjectActor", "whyshelf_core::domain::actor::Actor"]
serves: ["REQ-9"]
title: "The actor is never read from a committed file"
---
Who judges is the actor given on the command line, else the environment variable WHYSHELF_ACTOR, else the user.name Git gives in the project; a variable that is blank or not text counts as not set. No name is read from a file of the project, and with no name the judgement is refused.
