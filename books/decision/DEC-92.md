---
id: "01a1fb17-df61-7261-b4ee-e0a9ecdf00fd"
key: "DEC-92"
issues: ["#132"]
rejected: ["Read the other branches or open pull requests to take a key no one has: check would then depend on what is not on the shelf it checks", "A range of numbers per branch: every branch would need one given out, and a book's keys would stop telling the order documents came in", "Give the key at merge: a document has no name to link or talk about until it lands"]
related: ["DEC-28", "DEC-36", "DEC-4", "DEC-13"]
revisit: "When renumbering one document at a time with rekey (DEC-94) becomes routine work, so that preventing a key taken twice is worth its cost"
rustdoc: ["whyshelf"]
serves: ["REQ-39"]
title: "A key taken twice is repaired, not prevented: whyshelf reads only its own shelf, and the practice for several branches is one section of the manual"
---
Two branches can take the same key; nothing prevents it, and it is found and repaired. whyshelf reads only the shelf it is run on, never another branch or an open pull request, so check holds only what that shelf contains. A key taken twice in files of different names is found by check on the result of the merge, before it lands (in one file, a conflict stops the merge first), and repaired by renumbering one document, which rekey does (DEC-94); the identity stays, and the approvals hold, because the hash covers neither the key, the place nor the links (DEC-4). The practice is stated once, in the manual's section Working from several branches; the README section In CI says how CI checks pull requests on the result of their merge, and CONTRIBUTING says how this repository renumbers.
