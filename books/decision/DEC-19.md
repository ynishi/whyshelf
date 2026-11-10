---
id: "01a1c4ba-1465-7320-b3cb-05827d35ef01"
key: "DEC-19"
rejected: ["Judge one document per call: every call reads the whole project again"]
related: ["DEC-1", "DEC-8"]
rustdoc: ["whyshelf_core::app::shelf::ShelfApp::judge_all", "whyshelf_core::app::shelf::ShelfApp::same_meaning", "whyshelf_core::app::shelf::ports::LedgerAppend", "whyshelf_core::app::shelf::ports::DocWrite", "whyshelf_core::app::shelf::ports::ShelfRead", "whyshelf_core::domain::shelf::build::Shelf::claim_key"]
serves: ["REQ-5"]
title: "A run of judgements is decided whole before anything is written, and a read creates nothing"
---
Several judgements are decided in order on one reading, each on what the ones before it left, and are written only when all are accepted; a refused one writes none of the run. A port that fails part way through the writes can leave the first of them written, each a sound record of its own. A new document takes a key no file of its book writes, valid or not; a file whose front matter cannot be read is not counted. Reading commands create no file and no directory.
