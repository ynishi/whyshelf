---
id: "01a1c332-3988-7cda-b185-14a038a1dfdf"
key: "REQ-5"
grounds: ["F-1"]
title: "Commands that only answer write nothing"
---
Only `judge` (one new record per decision), `new` (one new document, never over an existing file) and `fmt` (a document's file rewritten in the one form, never what it reads as) write. Every other command only reads, and creates no file or directory.
