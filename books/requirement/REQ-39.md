---
id: "01a1fb17-d9d0-7550-ba37-441b6e4286ab"
key: "REQ-39"
grounds: ["F-58"]
issues: ["#132"]
title: "Documents can be added to one book from several branches at once, and a key taken twice is found before the merge and repaired without losing approvals"
---
Several people or agents can add documents to one book on branches of their own at the same time. When two branches take the same key, check on the result of merging one into the other reports it before that result lands, whatever Git does with the files. Renumbering the branch's document repairs it, and every approval of either document still holds afterwards. The practice is written where a user of whyshelf reads it.
