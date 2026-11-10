---
id: "01a1c4ba-155d-70fa-a267-9044c79b42b7"
key: "DEC-22"
rejected: ["Keep the adapters in a module of the core: only a lint would keep the core from I/O and not the compiler", "Share one error module across the domain: it would turn around the direction between its modules"]
rustdoc: ["whyshelf_core", "whyshelf_core::app::shelf::ports", "whyshelf_infra"]
serves: ["REQ-10"]
title: "The core is a crate with no I/O dependency, and its ports are typed traits"
---
The rules and the use cases live in a crate that depends on no I/O crate; files, Git, the clock and identities are adapters in another crate. Each port is a trait with its own error type and no dynamic dispatch. The crate boundary is kept by the core's manifest, which names no I/O crate, and a test of the architecture fails when code crosses the layers.
