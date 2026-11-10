---
id: "01a1c60b-bb53-7733-93f1-bf8478b9df23"
key: "DEC-44"
issues: ["#38"]
rejected: ["Keep files as the one kind of artifact and add a resolver for other names beside it: two mechanisms, with files special", "Let a link with no space mean a file: files would be special again"]
related: ["DEC-6"]
rustdoc: ["whyshelf_core::domain::shelf::artifact::Space", "whyshelf_core::domain::shelf::artifact::ArtifactName", "whyshelf_core::domain::shelf::artifact::ArtifactKey", "whyshelf_core::domain::shelf::schema::ArtifactLinkKind", "whyshelf_core::app::shelf::ports::ArtifactSource", "whyshelf_infra::shelf::artifact", "whyshelf"]
serves: ["REQ-1", "REQ-10"]
title: "Everything outside the documents is an artifact named in a space, and a link names its space"
---
A link to artifacts names its space. The domain knows a space by its name only, and takes the spaces the adapters offer as a value, so a link with no space or an unknown one is refused. A name is opaque to the domain: whether a text is a name of its space is the adapter's answer, and a text that is not makes the document invalid. Each space has one adapter, registered by its name; file is one of them, with no default and no other way in.
