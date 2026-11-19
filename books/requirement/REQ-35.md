---
id: "01a1f2a3-70f9-7184-8e59-6b24e25f79b6"
key: "REQ-35"
grounds: ["F-54"]
issues: ["#104"]
title: "An approval can stand on what a Rust item promises, its declaration and its docs, so that an edit of a body leaves it fresh and an edit of what the item states makes it stale"
---
A decision or a verification can name Rust items by their path and stand on what each promises: its declaration (the signature, the fields, the bounds) and its docs as the generated documentation shows them. For a variant its fields count too, and for a module, a crate, a primitive type or a keyword only its docs. An edit of a function's body, of an inherent method or of an impl changes nothing of the type. The declaration of a type, as the generated documentation writes it, lists its public fields, its variants and, for a trait, the signatures of its items, and the declaration of a constant holds its value, so those are part of what that item promises; a member named on its own is its own promise. An edit of the declaration or of the docs, docs written for an item that had none, or docs taken away, makes the approval stale. An item whose page cannot be read as a promise is told as cannot be told for such a comparison, never as changed, and still counts as there for a link that compares presence only. Whether white space and blank lines count is the choice of the link's comparison (lines collapses white space and leaves out blank lines, bytes does neither), not of the reading of the item. A link that compares presence only stays as it was.
