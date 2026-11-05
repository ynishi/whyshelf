//! Showing an outcome: as a table, as JSON, as Markdown.
//!
//! An [`Outcome`] is first turned into a [`Doc`]: the typed output of the command
//! (see [`output`](crate::output)), the context line when the command says it, and
//! the cursor of the next window. One document renders in the three [`Format`]s: JSON
//! is the serialization of the output, and the two text formats write its
//! sections, so a command never has three renderers and the formats cannot disagree
//! about what a command says. Nothing here decides anything: a list is cut to the
//! window the caller asked for.

use std::collections::BTreeSet;
use std::fmt::Write;

use clap::ValueEnum;
use schemars::JsonSchema;
use serde::Deserialize;
use whyshelf_core::app::query::Field;
use whyshelf_core::app::view::ContextLine;

use crate::command::Outcome;
use crate::error::CliError;
use crate::output::{Block, Document, Section, Shape, output};

/// How a result is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Aligned columns for a terminal.
    #[default]
    Table,
    /// One JSON object, sections by name.
    Json,
    /// Headings, pipe tables and lists.
    Markdown,
}

/// The cut of the first table of a document that a caller asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Window {
    limit: Option<usize>,
    offset: usize,
    fields: Vec<String>,
}

impl Window {
    /// A window of `limit` rows after `cursor`, with only the columns named in
    /// `fields` (all of them when empty).
    ///
    /// The cursor is the one an earlier answer gave; it is opaque to the caller.
    pub fn new(
        limit: Option<usize>,
        cursor: Option<&str>,
        fields: Vec<String>,
    ) -> Result<Self, CliError> {
        let offset = match cursor {
            Some(text) => text
                .parse::<usize>()
                .map_err(|e| CliError::argument("cursor", e))?,
            None => 0,
        };
        Ok(Self {
            limit,
            offset,
            fields,
        })
    }
}

/// What a command says, before it is written in a format.
#[derive(Debug)]
pub struct Doc {
    document: Document,
}

impl Doc {
    /// Put the context line first: in a table or Markdown the line itself, in JSON a
    /// `context` object. The tool server does not use this, since it opens every
    /// answer with the line already. Only a status says it.
    pub fn with_context(mut self, context: &ContextLine) -> Self {
        self.document.output.with_context(context);
        self
    }

    /// Cut the first table to the window: the columns asked for, then `limit`
    /// rows after the cursor. The cursor of the next window is kept.
    pub fn window(mut self, window: &Window) -> Result<Self, CliError> {
        let Some(table) = self.document.output.table() else {
            return Ok(self);
        };
        if !window.fields.is_empty() {
            table.select(&window.fields)?;
        }
        if let Some(next) = table.cut(window.offset, window.limit) {
            self.document.next_cursor = Some(next.to_string());
        }
        Ok(self)
    }

    /// Write the document.
    pub fn render(&self, format: Format) -> Result<String, CliError> {
        match format {
            Format::Json => self.json(),
            Format::Table => Ok(self.text(false)),
            Format::Markdown => Ok(self.text(true)),
        }
    }

    /// The output, serialized. It goes through a JSON value so that the keys of an
    /// object come out in sorted order, as they always have.
    fn json(&self) -> Result<String, CliError> {
        let value = serde_json::to_value(&self.document)?;
        Ok(serde_json::to_string_pretty(&value)?)
    }

    fn text(&self, markdown: bool) -> String {
        let sections = self.document.output.sections();
        let titled = sections.len() > 1;
        let mut blocks: Vec<String> = Vec::new();
        if let Some(line) = self.document.output.context() {
            blocks.push(format!("{}\n", line.text()));
        }
        for section in &sections {
            let mut block = String::new();
            if titled && !section.title.is_empty() {
                if markdown {
                    let _ = writeln!(block, "## {}\n", section.title);
                } else {
                    let _ = writeln!(block, "{}:", section.title);
                }
            }
            block.push_str(&section_text(section, markdown));
            blocks.push(block);
        }
        if let Some(cursor) = &self.document.next_cursor {
            blocks.push(format!("next cursor: {cursor}\n"));
        }
        blocks.join("\n")
    }
}

fn section_text(section: &Section, markdown: bool) -> String {
    let mut out = String::new();
    match &section.block {
        Block::Table { columns, grid } => {
            if markdown {
                pipe_table(&mut out, columns, grid);
            } else {
                aligned_table(&mut out, columns, grid);
            }
        }
        Block::Fields(fields) => {
            let width = fields.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
            for (key, value) in fields {
                if markdown {
                    let _ = writeln!(out, "- **{key}**: {value}");
                } else {
                    let _ = writeln!(out, "{key:<width$}  {value}");
                }
            }
        }
        Block::Lines(lines) => {
            for line in lines {
                if markdown {
                    let _ = writeln!(out, "- {line}");
                } else {
                    let _ = writeln!(out, "{line}");
                }
            }
        }
    }
    out
}

fn aligned_table(out: &mut String, columns: &[&str], grid: &[Vec<String>]) {
    let widths: Vec<usize> = (0..columns.len())
        .map(|i| {
            grid.iter()
                .map(|row| row[i].chars().count())
                .chain([columns[i].chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut line = |cells: &[String]| {
        let text: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        let _ = writeln!(out, "{}", text.join("  ").trim_end());
    };
    let header: Vec<String> = columns.iter().map(|c| c.to_ascii_uppercase()).collect();
    line(&header);
    for row in grid {
        line(row);
    }
}

fn pipe_table(out: &mut String, columns: &[&str], grid: &[Vec<String>]) {
    let escape = |text: &str| text.replace('|', "\\|").replace(['\n', '\r'], " ");
    let _ = writeln!(out, "| {} |", columns.join(" | "));
    let _ = writeln!(out, "|{}", "---|".repeat(columns.len()));
    for row in grid {
        let cells: Vec<String> = row.iter().map(|c| escape(c)).collect();
        let _ = writeln!(out, "| {} |", cells.join(" | "));
    }
}

/// The JSON Schema of `--format json`, made from the types the JSON is made from.
///
/// An answer with some columns left out (`--fields`) has fewer keys in each row
/// than the schema lists.
pub fn output_schema() -> Result<String, CliError> {
    let schema = serde_json::to_value(schemars::schema_for!(Document))?;
    Ok(serde_json::to_string_pretty(&schema)?)
}

/// The document an outcome says.
pub fn document(outcome: &Outcome, fields: &BTreeSet<Field>) -> Doc {
    // A page of `find` carries the cursor of the page after it.
    let next_cursor = match outcome {
        Outcome::Found(page) => page.next().map(|cursor| cursor.to_string()),
        _ => None,
    };
    Doc {
        document: Document {
            output: output(outcome, fields),
            next_cursor,
        },
    }
}
