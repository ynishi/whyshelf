//! The `whyshelf` command (CLI, and the MCP server as `whyshelf mcp`): parses
//! arguments, wires the `whyshelf-infra` adapters into the `whyshelf-core`
//! use cases, prints the result.
//!
//! The argument parser's definition (clap's derive types, once added) is the
//! only description of the commands: help, the command list and validation
//! come from it, not from a second hand-kept table. Structured input is read
//! from a file argument, not pasted into the command line.

fn main() {}
