---
id: "01a1cad2-34df-74f6-b9c7-64f09ffc1dd4"
key: "REQ-27"
grounds: ["F-34", "F-38"]
issues: ["#79"]
title: "A verification can name the very tests that are its evidence"
---
A person can name, as the evidence of a verification, single test functions rather than whole files, so that an edit of another test leaves it fresh, an edit of its own test makes it stale, and a test removed breaks it; reading the tests runs no program of the repository's choosing.
