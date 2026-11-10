---
id: "01a1c60b-bf0d-70d9-a6b0-d454e6662c4d"
key: "DEC-46"
issues: ["#38"]
rejected: ["Rewrite the stored records with a space: a record's hash would change, and one ledger would hold two forms of the same history"]
related: ["DEC-1", "DEC-2"]
rustdoc: ["whyshelf_core::domain::shelf::ledger::ArtifactPin"]
serves: ["REQ-7", "REQ-2"]
title: "A pin stored before spaces reads as a file, and stays as stored"
---
A pin, or a declaration of the same meaning, written with a path and no space reads as an artifact of the space file with that name, and is written back exactly as it was stored, so its record's hash does not change. A new one writes its space and its name.
