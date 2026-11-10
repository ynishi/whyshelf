---
id: "01a1c64b-b644-758a-85e6-34c76e900513"
key: "DEC-48"
issues: ["#39"]
rejected: ["Show the comment on the book's name line: a comment is no value of the declaration"]
related: ["DEC-11"]
rustdoc: ["whyshelf_core::domain::shelf::schema::BookDescription"]
serves: ["REQ-16"]
title: "A book may declare a description of one line"
---
A book's description is optional text of one line, kept as written. A blank one, one of invisible characters only, or one that holds a line break or another control character, is a problem of the declaration, so no answer holds a broken drawing.
