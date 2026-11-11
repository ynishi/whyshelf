---
key: "DEC-1"
title: "Write a new file, then rename it over the old one"
serves: ["REQ-1"]
rejected: ["Write in place and keep a backup copy"]
---
`notes save` writes the text to a new file in the same directory, flushes it to disk, and renames it over the old file. A rename within one directory replaces the file at once, so the file holds the old text or the new one, never a part of either.

A backup copy was rejected: while the file is written in place it is cut short, and the user has to know to restore the copy.
