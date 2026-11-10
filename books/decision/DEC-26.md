---
id: "01a1c532-68dc-71dd-a7c8-35e9618f81f6"
key: "DEC-26"
issues: ["#24"]
rejected: ["Run check and stale by hand before a pull request: nothing stops a merge when it is forgotten", "Make check fail on a stale approval: check answers whether the books are sound, and whether approvals hold is the question stale answers"]
related: ["DEC-33", "DEC-23"]
revisit: "When approving again what a change made stale costs more than the gate is worth"
rustdoc: ["whyshelf"]
serves: ["REQ-12", "REQ-2"]
title: "just check fails when the books are not clean or an approval does not hold"
---
stale --exit-code exits 1 when an approval does not hold, and 0 otherwise; without the flag it answers as before. just check, which is what CI runs, runs whyshelf check and whyshelf stale --exit-code on the repository, so a change merges only when its books are sound and every approval it touched was looked at again. An approval that only expired does not hold either, and fails the gate until it is approved again. Documents still pending pass it.
