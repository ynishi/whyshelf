---
key: "REQ-1"
title: "Saving never loses what was saved before"
grounds: ["F-1"]
---
When `notes save` fails, for any reason, the file holds what it held before the command ran. When it succeeds, the file holds the new text, whole.

How the command meets this is not decided yet. Until a decision serves REQ-1, `check` lists it as a warning.
