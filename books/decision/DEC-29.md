---
id: "01a1c55d-d380-7483-b160-18e3d1ecd75a"
key: "DEC-29"
issues: ["#30"]
rejected: ["Make the ledger's directory when it is first read: a read would change the project"]
replaces: ["DEC-19"]
rustdoc: ["whyshelf", "whyshelf_core::app::shelf::ports::LedgerRead::read"]
serves: ["REQ-5"]
title: "Reading commands create no file and no directory"
---
A command other than judge, new and fmt reads the project and leaves every file and directory as it found them, before the first approval as after it.
