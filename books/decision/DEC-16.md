---
id: "01a1c4ba-136c-718f-b63c-ca159f35530a"
key: "DEC-16"
rejected: ["Write the successor into the deprecation: it would go stale when the links change"]
related: ["DEC-5", "DEC-13"]
rustdoc: ["whyshelf_core::domain::shelf::freshness::Successors::of", "whyshelf_core::domain::shelf::build::Shelf::replacers_of", "whyshelf_core::app::shelf::Judgement::Deprecate", "whyshelf_core::app::shelf::ShelfError::NotASuccessor"]
serves: ["REQ-2", "REQ-3"]
title: "A successor is a fact of replaces links, judged on every read"
---
The successors of a deprecated document are the pending or approved documents with a replaces link to it; the chain is not followed. A deprecation records only that the document is to be replaced. A successor named with a deprecation is a check against that set and nothing more.
