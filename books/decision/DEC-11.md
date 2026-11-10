---
id: "01a1c4ba-11d9-7657-8910-6240e23765d9"
key: "DEC-11"
rejected: ["Allow floating-point fields: a measurement is an integer or a text as in a canonical form"]
related: ["DEC-3"]
revisit: "When a book needs a field type the five do not express"
rustdoc: ["whyshelf_core::domain::shelf::schema::Schema::validate", "whyshelf_core::domain::shelf::schema::SchemaProblem", "whyshelf_core::domain::shelf::schema::KeyPattern", "whyshelf_core::domain::shelf::schema::FieldType", "whyshelf_core::domain::shelf::schema::Cardinality"]
serves: ["REQ-1", "REQ-6"]
title: "A declaration of books is checked whole: one key grammar per book, typed fields, counted links"
---
The declaration is validated once and every problem is returned at once; nothing downstream checks it again. Each book has one key grammar (a prefix and a number or name, or a slug that never looks like an id), five field types, and links with a count and a meaning. Clashes that would make a key or a name ambiguous are refused.
