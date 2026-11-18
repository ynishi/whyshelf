---
id: "01a1e3a5-6ee3-7525-839f-9b046ea44760"
key: "DEC-75"
issues: ["#125"]
rejected: ["Links between the rows of a run only by keys given with --key: the caller would number the documents and meet the collisions new already settles", "Repeated arguments in one command line: their separation breaks easily, and many documents call for a file", "Write the valid rows and list the rest: links between the rows of the run would be cut part way; a run refused whole can be run again from the same file once its rows are put right", "Make the run as fast as one new by putting all its files on disk in one step: it changes how durable each file is, and is left to #133 (done in DEC-79)"]
related: ["DEC-74", "DEC-28"]
revisit: "When a run must also change documents that are there"
rustdoc: ["whyshelf_core::domain::shelf::build::Shelf::plan_new", "whyshelf_core::domain::shelf::build::NewPlan", "whyshelf_core::domain::shelf::build::PlanProblem", "whyshelf_core::app::shelf::ShelfApp::new_docs"]
serves: ["REQ-5"]
title: "new plans the documents of a run at once from a file of JSON lines: keys, names that link rows, and one judgement of all of them before anything is written"
---
new --from reads a file of JSON lines (or standard input), one document per line, whose shape whyshelf schema prints. The shelf is read once; one entry of the domain gives the keys of the run (a key asked for is claimed as by --key, the others take the next free numbers, passing over keys asked for), puts the key of a row for each name another row links to, refusing a name that no row has, that two rows have, or of a document of another book than the link takes, and judges every row as new judges one, with the links between rows resolved. A refused row makes the run write nothing and is named by its line. The documents are then written, those linked to by name first; a key found taken while writing is moved to the next free number, as for one new, each row giving up after its own 64 keys. A failure after a write lists the files written. new of one document and init plan through the same entry. Reading the shelf is paid once a run, and so is putting its files on disk (DEC-79).
