---
id: "01a1c811-cfe5-739d-9535-559effdfed85"
key: "DEC-54"
issues: ["#56"]
rejected: ["Read rustdoc's JSON: it is unstable and needs a nightly toolchain", "Parse the source with syn: re-exports and items made by macros would not be found", "Ask cargo metadata where the build directory is: a repository's toolchain file would choose the program a reading command runs", "Compare an item's docs, not only its presence: every edit of a doc comment would make its decisions stale"]
related: ["DEC-44", "DEC-45", "DEC-53"]
revisit: "When rustdoc's HTML changes its layout, or its JSON is stable"
rustdoc: ["whyshelf_infra::shelf::artifact::rust_item", "whyshelf_core::domain::shelf::artifact::ArtifactReading::there", "whyshelf"]
serves: ["REQ-13", "REQ-1"]
title: "Rust items are a space resolved in the built docs, and rustdoc is the preset that pins their presence"
---
The space rust-item names a public Rust path, crate first, an item or a member of a type last. Its adapter reads the HTML cargo doc writes in the project's build directory, found without running any program: CARGO_TARGET_DIR, else build.target-dir of the cargo settings at the root, else the directory beside the root's Cargo.toml. A path resolves to an item's page, a module's index, a member's anchor, or a re-export followed while it stays in the docs after links are resolved. A crate whose docs are not built, whose index is not rustdoc's, or a page that cannot be read or is too large is cannot tell, never absent; a path through a private module is absent, as it is not public. The docs are read as they are, so this repository's rust-doc recipe removes the old ones before it builds; the limits of reading them (glob re-exports of a public module, members through a type alias and hidden items read as absent, members through Deref or a blanket impl as there) are written on the adapter's module. The adapter offers the preset rustdoc: space rust-item, compare presence, means depends, so editing an item's docs makes nothing stale and an item that goes away breaks the decisions that name it; another comparison cannot be told. The domain gains only a reading that an artifact is there, with no content.
