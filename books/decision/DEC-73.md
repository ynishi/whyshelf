---
id: "01a1e170-61cb-728f-adea-c80c24e72315"
key: "DEC-73"
issues: ["#126"]
rejected: ["Keep the words in the adapter and give the domain words only for the messages: the same table of words would sit in four places", "Rebuild the messages in the command line from the variants: the answer of check in JSON and the message would disagree, and the domain would no longer own its problems", "Write the words by hand in each message: the tables of the adapter and of map stay, and nothing keeps them equal", "Name the declaration file in the message of an undeclared entry: where the file is kept is the adapter's, and the domain would learn it; the message names the book", "Write what each count allows in the comment init writes: a copy by hand of the domain's words; a problem of a count says what it allows when it matters"]
related: ["DEC-11", "DEC-47"]
revisit: "When a problem should also say where the declaration file is, or which count would admit what was written"
rustdoc: ["whyshelf_core::domain::shelf::schema::Cardinality", "whyshelf_core::domain::shelf::schema::ScalarType", "whyshelf_core::domain::shelf::schema::FieldType", "whyshelf_core::domain::shelf::schema::Propagation", "whyshelf_core::domain::shelf::schema::SchemaProblem", "whyshelf_core::domain::shelf::build::DocProblem"]
serves: ["REQ-1", "REQ-8"]
title: "The words of a declaration are read by the domain, and a problem says a count, a type and a book as the declaration says them"
---
A count, a type and a meaning are kept in a declaration as written, and the domain reads each with one reader, whose words come from the values of the type: a word the type does not have is a problem of the declaration, naming the book and the entry and listing the words it takes, told with every other problem of the declaration (exit 1), not a failure to read the file. A count left out is many. The problems of a document say the words of the declaration: a count with what it allows ("optional" (none or one)), a type by its word, a written value by its kind (a text, a whole number, a truth value, a list), and an undeclared entry with its book and that it is to be declared there. map says the same words.
