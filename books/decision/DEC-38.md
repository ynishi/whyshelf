---
id: "01a1c55d-f0b8-7358-973e-9c8cf9148148"
key: "DEC-38"
issues: ["#15", "#30"]
rejected: ["Say only that the value is not valid: a person cannot see what it was read as"]
related: ["DEC-12"]
revisit: "When every such value of a file is told, not only the first (#21)"
rustdoc: ["whyshelf_core::domain::shelf::build::UnheldKind", "whyshelf_core::domain::shelf::build::UnheldItem"]
serves: ["REQ-8"]
title: "A value a front matter does not hold is told with the kind it was read as"
---
A value of a kind a front matter does not hold (such as a mapping, a null or a number too large) makes the document invalid with the kind it was read as, and the usual way to write it where there is one.
