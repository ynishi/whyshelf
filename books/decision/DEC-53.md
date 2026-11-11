---
id: "01a1c7e2-6f31-74e1-8aa9-4de2bd6986d2"
key: "DEC-53"
issues: ["#54"]
rejected: ["Build a default space into the core: files would be special again", "Keep the preset's name in a pin: editing the preset would change how an old pin reads", "Expand presets in the adapter that reads the TOML: the rule of precedence would be the adapter's"]
related: ["DEC-44", "DEC-10", "DEC-47"]
revisit: "When a space offers parts of an artifact to compare (facets), and a preset must name one"
rustdoc: ["whyshelf_core::domain::shelf::schema::expand", "whyshelf_core::app::shelf::ports::ArtifactSource::presets", "whyshelf"]
serves: ["REQ-20", "REQ-1"]
title: "Presets and a default are expanded from the declaration before it is checked, and only their values are kept"
---
A preset is a named part of a link to artifacts: its space, its comparison with its ignored prefixes as one value, and its meaning; a link that writes a comparison takes none of its preset's ignored prefixes unless it writes them too. The adapters offer built-in presets for their own spaces, as values through the port; a project declares its own, which may build from another (from its own name, the built-in of that name) and, written without from, replaces a built-in of the same name whole. A link that names a preset takes its values, and each value the link writes wins; a default names the preset a link to artifacts takes when it names neither a preset nor a space, and without one there is no default. One pure function expands the declaration before it is checked, and only the expanded declaration is checked, answered and used, so no pin, cause or notice holds a preset's name; map answers which preset each link came from. A preset's own values are checked on the preset, used or not, and a problem of a preset is a problem of the declaration that names where it came from; the problems of expansion and of checking what expanded are answered together.
