---
id: "01a1c55d-d99a-72ad-86a3-e75931d0b50a"
key: "DEC-31"
issues: ["#28", "#30"]
rejected: ["Leave a wrong argument to the parser's text: a caller reading JSON would get something else for that error alone"]
replaces: ["DEC-20"]
rustdoc: ["whyshelf"]
serves: ["REQ-4"]
title: "Every answer is one versioned JSON that only grows, a wrong argument included"
---
Every answer is one JSON document with a version field. A wrong argument is answered with the error of a request, its text also on stderr; only --help and --version answer in text. Within a version the shape only grows.
