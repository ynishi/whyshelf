---
id: "01a1c5c5-7849-7632-b126-5a6b72f508b0"
key: "DEC-43"
issues: ["#36"]
rejected: ["Leave case to the file system: one approval would hold on one system and break on another", "Ask Git which paths it tracks: a project outside version control, or a file not yet committed, would have none"]
related: ["DEC-6"]
rustdoc: ["whyshelf_core::app::shelf::ports::ArtifactSource::read_artifacts", "whyshelf_infra::shelf::git", "whyshelf"]
serves: ["REQ-13", "REQ-2"]
title: "A path names a file only as written, ASCII case included, on every system"
---
A path that differs from the name of an entry of its directory in ASCII case only names no file, so justfile names no file Justfile on a file system that does not tell case apart, as on one that does and for Git. The rule is the port's, and the adapter keeps it. A name that differs in a non-ASCII letter's case, or that the system stores in another form (macOS keeps some non-ASCII names decomposed), is left to the system, and a directory that cannot be listed says nothing against a path that opened.
