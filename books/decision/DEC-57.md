---
id: "01a1c8ad-9bab-71bd-a155-f52d9ae7d16d"
key: "DEC-57"
issues: ["#64"]
rejected: ["A text output in place of the JSON, behind a flag: every command answers one typed JSON document", "Read a date given to --since in the machine's time zone: the same command would answer differently on two machines"]
related: ["DEC-55", "DEC-47"]
serves: ["REQ-21", "REQ-4"]
title: "stats carries a drawing a person reads, made from the answer alone in the output of the command line"
---
The answer of whyshelf stats keeps its JSON and carries, as map does, a drawing: a plain text summary in five headed sections, approvals per book, the documents approved again most often, pending time per book in hours and minutes, runs with the largest at their time in RFC 3339 UTC, and the origins most often under an approval made again, an artifact outside the space file written as its space and name; a list drawn short says how many it holds in all, and columns are aligned by display width. The output of the command line makes it from the answer alone, reading no clock and no time zone, so the same ledger gives the same text; the core is unchanged. A time given to --since or --until is either an RFC 3339 time with an offset or Z, or a date written YYYY-MM-DD, the start of that day in UTC; anything else, a time with no offset included, is refused rather than read as its date, as their help says.
