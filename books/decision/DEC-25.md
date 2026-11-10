---
id: "01a1c502-6097-722a-b119-3a584d66a0b3"
key: "DEC-25"
issues: ["#17"]
rejected: ["Check the wording with a linter or a fixed vocabulary: a statement that is well formed and untrue passes it", "Give the reviewer a summary of the change: the review stays inside the summary, which a blind review of the change in pull request #22 showed by finding six more problems", "Let whyshelf judge meaning: its rules are hashes and links, and a judgement of meaning would be guessed", "Ask for the review through a gate of whyshelf: whyshelf has no gates yet", "Keep each review's report beside the documents: a report is long and belongs with the change it reviewed, in its pull request, and the approval's reason names the review"]
related: ["DEC-15", "DEC-9"]
revisit: "When checking that each rustdoc path a decision names still resolves can be done by whyshelf, or when whyshelf has gates"
serves: ["REQ-13"]
title: "The meaning of the books is checked by a blind review that starts from whyshelf, not by whyshelf"
---
whyshelf keeps what was decided and whether its hashes still hold; it does not judge whether a statement is true. A change that touches a decision, a verification or the code they point at is reviewed by a person or an AI agent who is given only the issue and the branch, starts from whyshelf, and reads the documents against the code; the review and what it found are said in the pull request, and the approval's reason names it. How to ask for such a review is written once, in CONTRIBUTING.md under Review.
