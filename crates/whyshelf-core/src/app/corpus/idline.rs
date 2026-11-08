//! Putting an `id:` into the bytes of a file, and changing nothing else.
//!
//! An import gives a document, or the items of a Design, the identity they lack. The
//! file's bytes are the person's: the new bytes are the old ones with a line inserted,
//! and every other byte stays, line ends included (a file written with `\r\n` gets a
//! `\r\n`). Reading the file as a document is the adapter's; here only the shape that
//! the adapter reads is relied on: a front matter between two `---` lines, whose
//! `items:` key holds a block sequence of mappings.
//!
//! | file | the `id:` line goes |
//! |---|---|
//! | a front matter | right after the opening `---` |
//! | no front matter | in a new front matter put before the first byte |
//! | a Design's item entry (`  - number: 2`) | right after the first line of the entry, at the indent of its keys |
//!
//! An item entry the shape does not cover (a flow mapping, `- {number: 2}`) is not
//! changed: [`with_item_ids`] says so with `None` when it could place no `id:` at all.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::{DocId, ItemId, ItemNumber};

/// The line end the file uses, judged from its first line.
fn eol_of(first: &str) -> &'static str {
    if first.ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn is_fence(line: &str) -> bool {
    line.trim_end() == "---"
}

/// The bytes of a file with the `id:` of a document put in. See the module
/// documentation.
pub(super) fn with_doc_id(bytes: &[u8], id: DocId) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let first = text.split_inclusive('\n').next().unwrap_or("");
    let eol = eol_of(first);
    let mut out = Vec::with_capacity(bytes.len() + 64);
    if is_fence(first) {
        out.extend_from_slice(&bytes[..first.len().min(bytes.len())]);
        if !first.ends_with('\n') {
            out.extend_from_slice(eol.as_bytes());
        }
        out.extend_from_slice(format!("id: {id}{eol}").as_bytes());
        out.extend_from_slice(&bytes[first.len().min(bytes.len())..]);
    } else {
        out.extend_from_slice(format!("---{eol}id: {id}{eol}---{eol}").as_bytes());
        out.extend_from_slice(bytes);
    }
    out
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// The number an item entry says it has, from its lines.
fn number_in(entry: &[&str], content_indent: usize) -> Option<ItemNumber> {
    let first = entry.first()?.trim_start().strip_prefix('-')?.trim_start();
    let keyed = std::iter::once(first).chain(
        entry
            .iter()
            .skip(1)
            .filter(|line| indent_of(line) == content_indent)
            .map(|line| line.trim_start()),
    );
    keyed
        .filter_map(|line| line.strip_prefix("number:"))
        .find_map(|value| {
            let digits = value.trim().trim_matches(['"', '\'']);
            ItemNumber::new(digits.parse().ok()?).ok()
        })
}

/// Whether an item entry has an `id:` of its own.
fn has_id(entry: &[&str], content_indent: usize) -> bool {
    let first = entry
        .first()
        .and_then(|line| line.trim_start().strip_prefix('-'))
        .map(str::trim_start);
    first.is_some_and(|line| line.starts_with("id:"))
        || entry
            .iter()
            .skip(1)
            .any(|line| indent_of(line) == content_indent && line.trim_start().starts_with("id:"))
}

/// The bytes of a Design's file with an `id:` put into each item entry that has none
/// and whose number is in `ids`, with the numbers of the entries that got one (an entry
/// the shape does not cover gets none, so they may be fewer than `ids`). `None` when the
/// file is no text, has no front matter with an `items:` list of block entries, or no
/// entry was changed.
pub(super) fn with_item_ids(
    bytes: &[u8],
    ids: &BTreeMap<ItemNumber, ItemId>,
) -> Option<(Vec<u8>, BTreeSet<ItemNumber>)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let opening = lines.first().copied()?;
    if !is_fence(opening) {
        return None;
    }
    let eol = eol_of(opening);
    let closing = lines.iter().skip(1).position(|line| is_fence(line))? + 1;
    let items_at = lines[1..closing]
        .iter()
        .position(|line| line.trim_end() == "items:")?
        + 1;

    // The entries of the block: from the first line that starts one, until a line that
    // is not part of the sequence.
    let entry_indent = lines[items_at + 1..closing]
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| indent_of(line))?;
    let content_indent = entry_indent + 2;
    let starts_entry =
        |line: &str| indent_of(line) == entry_indent && line.trim_start().starts_with("- ");
    let mut ends = closing;
    for (offset, line) in lines[items_at + 1..closing].iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if indent_of(line) < entry_indent
            || (indent_of(line) == entry_indent && !starts_entry(line))
        {
            ends = items_at + 1 + offset;
            break;
        }
    }

    let mut out = String::with_capacity(text.len() + 64 * ids.len());
    let mut placed = BTreeSet::new();
    for (idx, line) in lines.iter().enumerate() {
        out.push_str(line);
        if idx <= items_at || idx >= ends || !starts_entry(line) {
            continue;
        }
        let stop = lines[idx + 1..ends]
            .iter()
            .position(|next| starts_entry(next))
            .map_or(ends, |next| idx + 1 + next);
        let entry: Vec<&str> = lines[idx..stop].to_vec();
        let rest = line
            .trim_start()
            .strip_prefix('-')
            .map_or("", str::trim_start);
        if rest.is_empty() || rest.starts_with('{') || rest.starts_with('[') {
            continue;
        }
        if has_id(&entry, content_indent) {
            continue;
        }
        let Some((number, id)) = number_in(&entry, content_indent)
            .and_then(|number| ids.get(&number).map(|id| (number, id)))
        else {
            continue;
        };
        if !line.ends_with('\n') {
            out.push_str(eol);
        }
        out.push_str(&format!("{}id: {id}{eol}", " ".repeat(content_indent)));
        placed.insert(number);
    }
    (!placed.is_empty()).then(|| (out.into_bytes(), placed))
}

#[cfg(test)]
mod tests;
