---
id: "01a1c4e9-62d4-72e8-bf6a-0b974af9ea91"
key: "DEC-24"
issues: ["#14"]
rejected: ["Append when the same entry is given twice: whether one value is split at its commas would then depend on how often it is given", "Read a list as JSON or YAML on the command line: quoting inside quoting for every list"]
related: ["DEC-8"]
rustdoc: ["whyshelf_core::app::shelf::NewDocRequest"]
serves: ["REQ-1"]
title: "A list item is given one at a time with +=, as written"
---
An entry name+=text appends one item to a list field or a link, as written with its commas, white space around it left out as for a split item; name=a,b keeps its meaning, a value split at its commas. Both may be given for one name: the items of the split value come first, then each item given with += in the order given. An item for a field that is no list, and an empty item, are refused as a wrong request, and name= given twice is still refused.
