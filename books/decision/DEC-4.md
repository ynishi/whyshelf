---
id: 01a1c332-3cd5-7944-ac64-25bd8640a4ca
key: DEC-4
title: "A document's hash covers its book, fields and body only"
serves: [REQ-2]
related: [DEC-2]
rustdoc: [whyshelf_core::domain::shelf::doc::DocHash]
---
The key, the place of the file and the links are left out: renaming or moving a document keeps its approval, and a change of links is told separately.
