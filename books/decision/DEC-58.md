---
id: "01a1c8cf-dfdb-771f-bea4-6b65c56de737"
key: "DEC-58"
issues: ["#66"]
rejected: ["Refuse the run: a run named a stale document and a fresh one would write neither", "Write and warn: the record that adds nothing would still be counted as an approval made again"]
related: ["DEC-27", "DEC-51"]
rustdoc: ["whyshelf_core::domain::shelf::ledger::repeated", "whyshelf_core::app::shelf::ShelfApp::judge_all"]
serves: ["REQ-22", "REQ-21"]
title: "judge approve leaves out a document whose standing approval a new one would only repeat, unless --again"
---
A new approval repeats the standing one when the standing decision is an approval and the frame taken now equals the frame it pinned, with the same expiry: every document pin by link, identity, key and hash, every artifact pin by link, space and name, comparison, and present with its hash or absent, and the document's own hash. Addresses and the revision, which freshness never reads, and who approves and why are not compared, and freshness is not asked: an approval stale only through an upstream repeats, and one fresh only by a declared same meaning has a new hash and does not. The domain decides it once, in the ledger beside the frame. judge approve writes no record for such a document, lists it under left_out with the reason repeats and the approval that stands, warns of it on stderr, and judges and writes the rest of the run; a run where every document is left out writes nothing, asks for no actor and exits 0. --again writes it anyway; an expiry not after now is refused before any of this is asked. approve --stale leaves out the same way a document stale only through an upstream that the run approved again, and such a document holds back nothing downstream; the other verbs are unchanged.
