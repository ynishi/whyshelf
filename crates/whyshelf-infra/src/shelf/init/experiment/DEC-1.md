---
key: "DEC-1"
title: "Write the whole text in one buffered write"
serves: ["REQ-1"]
rejected: ["Save in the background and return at once"]
---
`notes save` builds the whole text in memory and writes it to the file in one buffered write, not one small write per note.

Saving in the background was rejected: the command would return before the file is written, and a save that fails could not be told to the user.

When the code is written, name its file in `code`: an edit of it leaves this decision as it is, and a removal or a rename breaks it.
