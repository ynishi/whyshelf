---
id: "01a1c877-8fbd-7353-9e60-cfe043fb39a9"
key: "DEC-56"
issues: ["#60"]
rejected: ["Keep one file and pin each verification to its test functions: no space names a test function yet", "Name no shared helpers in tests: an edit of a helper would change what a test checks with no approval stale"]
related: ["DEC-55"]
serves: ["REQ-13"]
title: "Each feature's end-to-end tests are a file of their own, and a guard keeps one file from serving many verifications"
---
The end-to-end tests of whyshelf-cli are one file per feature under crates/whyshelf-cli/tests, and each verification names the files of its own feature. The helpers the files of one kind of project share are one module, common/mod.rs, which holds no test and which every verification over those files also names, as a test means what its helpers do; a helper used by one feature stays in its file, and a file whose project is made another way keeps its own. A test reading this repository's verifications fails when a file there other than common/mod.rs is named by more than two, so an edit of one feature's tests leaves the approvals of the others fresh. Every temporary directory these tests make by hand holds the name of its test or feature and a counter, so tests run in parallel never share one.
