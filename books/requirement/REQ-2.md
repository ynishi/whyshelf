---
id: "01a1c332-378a-756b-a5b1-fd2da76f8d8c"
key: "REQ-2"
title: "Freshness is derived on every read and never stored"
---
Whether an approval still holds, and where it starts to fail, is computed from the documents and the ledger each time it is asked.

Records written by an earlier build are read back unchanged: a change of the code never rewrites, drops or reinterprets a decision already made.
