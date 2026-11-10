---
id: "01a1c45a-279c-7672-ac87-9afd9c344dc8"
key: "DEC-10"
rejected: ["Let the adapter choose what of a file it hashes: the rule would live outside the domain and change with the adapter", "Strip comments with a parser per language: one parser per language and a second notion of equal", "Compare a pin with the hash now under the link's new comparison: the two were made differently and would read as a change of the file"]
related: ["DEC-6", "DEC-9"]
revisit: "When a prefix of a line is not enough (comments at the end of a line or blocks): add a comparison of one language as another kind"
rustdoc: ["whyshelf_core::domain::shelf::artifact::Comparison", "whyshelf_core::domain::shelf::freshness::Cause::ArtifactComparisonChanged"]
serves: ["REQ-2"]
title: "What of a file counts is declared per link and kept in each pin"
---
A link to artifacts declares what of a file counts: every byte, or its lines without white space, blank lines and the lines that start with a declared prefix. The domain makes every hash of a file from that comparison, and each pin keeps the comparison its hash was made with; two hashes are compared only when made the same way, and a link whose comparison changed since says so as its own cause.
