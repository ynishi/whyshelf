---
id: "01a1efb4-4d1c-714f-bfb4-86da4f4fac78"
key: "REQ-32"
grounds: ["F-50"]
issues: ["#133"]
title: "The files a run writes are each whole, and once it answers that it wrote them they survive a power loss, at the cost of one flush for the run"
---
A run of new or init writes each file whole or not at all, so that a failure of the run leaves no part of a file behind. When it answers that it wrote its files they are on disk, and a run that stopped part way puts on disk the files it wrote before it answers which they are. The cost of making a run durable is about one flush of the drive, not one for each file, so that writing many documents at once does not cost a flush a row.
