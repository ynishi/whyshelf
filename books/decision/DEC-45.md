---
id: "01a1c60b-bc99-7728-b8d7-deea06f9c49b"
key: "DEC-45"
issues: ["#38"]
rejected: ["Count what cannot be told as absent: an index not built would read as evidence gone", "Count what cannot be told as present: an approval would stand on what nobody saw"]
related: ["DEC-6", "DEC-42"]
rustdoc: ["whyshelf_core::domain::shelf::artifact::ArtifactReading", "whyshelf_core::domain::shelf::ledger::FrameRefusal", "whyshelf_core::domain::shelf::freshness::Notice", "whyshelf"]
serves: ["REQ-12", "REQ-2"]
title: "A name resolves to present, absent, cannot tell or not a name"
---
Every name asked about has one answer from its adapter: present, with a hash per comparison and an address when it can say; absent; cannot tell now, with why; or not a name, with why. An approval that would pin a name that cannot be told now is refused. A pinned name that cannot be told later is a notice and changes no freshness, and check lists such names under unknown and stays clean.
