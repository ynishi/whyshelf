---
id: "01a1c55d-ce54-705b-a4fb-17382395564a"
key: "DEC-27"
issues: ["#30"]
rejected: ["Judge one document per call: every call reads the whole project again"]
related: ["DEC-1"]
replaces: ["DEC-19"]
rustdoc: ["whyshelf_core::app::shelf::ShelfApp::judge_all", "whyshelf_core::app::shelf::ShelfApp::same_meaning", "whyshelf_core::app::shelf::ports::LedgerAppend"]
serves: ["REQ-5"]
title: "A run of judgements is decided whole before anything is written"
---
Several judgements are decided in order on one reading, each on what the ones before it left, and are written only when all are accepted; a refused one writes none of the run. A port that fails part way through the writes can leave the first of them written, each a sound record of its own.
