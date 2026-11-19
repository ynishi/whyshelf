---
id: "01a1f44e-7ea5-700b-90ca-49a9faa991ca"
key: "DEC-86"
issues: ["#146"]
rejected: ["Keep the later one: a space registered a second time replaced the adapter registered before it, and of two built-in presets of one name the one of the later space was kept, so one adapter took the place of another with no word", "Let the registry of the adapters in the infrastructure refuse a collision: the rule would sit with the list of the adapters, apart from the other rules of the dispatch in the core"]
related: ["DEC-53", "DEC-84", "DEC-44"]
revisit: "When a project needs two adapters of one space (a second reader of a space chosen by a setting), or to put an adapter of its own in the place of a built-in one"
rustdoc: ["whyshelf_core::app::shelf::Spaces::new", "whyshelf_core::app::shelf::DispatchError", "whyshelf_core::app::shelf::SpaceAdapter::space", "whyshelf_core::app::shelf::SpaceAdapter::presets", "whyshelf_infra::shelf::Project::new"]
serves: ["REQ-36", "REQ-20"]
title: "Two adapters that name one space, or offer a built-in preset of one name, are refused when the dispatch is built, and neither replaces the other"
---
The dispatch of the core keys the adapters by the space each names and gathers the built-in presets each offers (DEC-84). When it is built, a space that two adapters name, a built-in preset of one name that two adapters offer, a built-in preset whose space is not the space of the adapter that offers it, and a text an adapter names as its space that is no space are each an error, and the shelf is not opened: a command that meets one fails with exit 3 and reads nothing. Before, a space registered a second time replaced the adapter registered before it, and of two built-in presets of one name the one of the later space, in the order of the names of the spaces, was kept, with no word of either. DEC-53 decides that the adapters offer built-in presets for their own spaces and that a preset a project declares replaces a built-in of the same name whole; this decides what it left open, two adapters that collide. The adapters whyshelf builds in collide in none, and a test holds them to it.
