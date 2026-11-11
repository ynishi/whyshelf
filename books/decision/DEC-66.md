---
id: "01a1cb06-d455-710f-8a4e-5bd7fa7cd3c8"
key: "DEC-66"
issues: ["#82"]
rejected: ["Render templates with variables or conditionals: two to four built-in templates read best as whole files", "Add a list command: the names and their descriptions are in init --help, as small tools do", "Fetch templates from a registry or git: a template is data in the binary, and init needs no network"]
related: ["DEC-61"]
rustdoc: ["whyshelf_infra::shelf::init::Template"]
serves: ["REQ-24"]
title: "init chooses one of a closed set of templates by name, each a complete set of files"
---
whyshelf init --template <name> takes one of a closed set kept as data in the infra, Template: minimal, the default; sample, the four books with a chain of documents; and qa-first, the four books with two rules loosened and marked, a verification may check no decision yet and a requirement no decision serves is a warn gap, whose documents are written before any decision. Each template is a complete set of files with no conditionals, its documents valid for its own declaration and in the one form, so check is clean right after init. --sample is a shorthand for --template sample, and giving both is refused; an unknown name is refused naming the templates there are, and init --help lists each with a line. The core is unchanged: the template only chooses the declaration text and the documents init is given, and next still names the approval of the documents none of them stands on.
