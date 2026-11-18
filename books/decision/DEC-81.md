---
id: "01a1f0ce-5b0a-7405-b5a1-fb50213d286a"
key: "DEC-81"
issues: ["#103"]
rejected: ["Sync each record as it is appended: a flush of the drive for each record, so that a declaration of many names would cost a flush a name", "Leave the records of judge moved --prefix alone outside the run and keep the other use cases as they were: two ways to write a record, and the new one would be the one that leaves some records on a failure part way", "Keep a record whose write failed: a file left behind is read by every read of the ledger as a record that cannot be read", "Put the file by file sync of fmt on the same path in this change: it has the same cause and is left for its own issue"]
related: ["DEC-79"]
revisit: "When fmt writes its files the same way, or a platform gives a cheaper flush that is still durable"
rustdoc: ["whyshelf_core::app::shelf::LedgerAppend", "whyshelf_core::app::shelf::FileSync", "whyshelf_core::app::shelf::ShelfError"]
serves: ["REQ-33"]
title: "Every record of the ledger is written inside RunWrites::run, whole or removed, and the files of a run, records included, are put on disk once at its end"
---
A record of the ledger is a file of its own, written the way any new file of a run is: create-only, whole, and removed when its write fails, with no sync of its own. The six use cases that write records (judge and approve, same meaning, moved, exempt, withdraw) and judge moved --prefix make them only inside RunWrites::run, by RunWrites::record, which keeps the path the ledger says it wrote at (LedgerAppend::append); the run puts every path it kept on disk once at its end, through FileSync, and a run that stops part way answers which records were written (PartWay), and one that wrote all and could not put them on disk answers NotDurable, the records whole and named. The one call of append is in the module of the run, and a test looks for another.
