---
id: 01a1c332-3988-7cda-b185-14a038a1dfdf
key: REQ-5
title: "Commands that only answer write nothing"
grounds: [F-1]
---
Only `judge` (one new record per decision) and `new` (one new document, never over an existing file) write. Every other command only reads, and creates no file or directory.
