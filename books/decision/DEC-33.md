---
id: "01a1c55d-dfd2-7125-a8b3-7f3af22d2fce"
key: "DEC-33"
issues: ["#30"]
rejected: ["A catch-all class: an error added later would take it without anyone choosing"]
replaces: ["DEC-20"]
rustdoc: ["whyshelf", "whyshelf_core::app::shelf::ErrorClass", "whyshelf_core::app::shelf::ShelfError::class"]
serves: ["REQ-4"]
title: "Four exit statuses over three classes of error"
---
The exit status is 0 answered, 1 when check found something wrong, when the model refused a judgement, or when stale --exit-code found an approval that does not hold (DEC-26), 2 a wrong request and 3 a failed run. Every error belongs to one of three classes (request, refused, failed), with no catch-all, so an error added without a class does not build.
