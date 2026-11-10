---
id: "01a1c4ba-14b4-77f7-9af0-acbe8896080b"
key: "DEC-20"
rejected: ["Group the commands under one noun: a word to type with one kind of thing to act on", "Keep the old command names as aliases: one thing would have two names", "Write the schema apart from the output: the two would drift apart"]
revisit: "When a second kind of thing needs commands of its own"
rustdoc: ["whyshelf", "whyshelf_core::app::shelf::ErrorClass", "whyshelf_core::app::shelf::ShelfError::class"]
serves: ["REQ-4"]
title: "Commands are flat verbs whose answers are one versioned JSON that only grows, with four exit statuses"
---
The commands are verbs at the top level, with judge as the one group because its verbs take different flags. Every answer is one JSON document with a version field; within a version the shape only grows, and schema prints the JSON Schema made from the same types. The exit status is 0 answered, 1 when check found something wrong or the model refused a judgement, 2 a wrong request and 3 a failed run. Every error belongs to one of three classes (request, refused, failed), with no catch-all, so an error added without a class does not build.
