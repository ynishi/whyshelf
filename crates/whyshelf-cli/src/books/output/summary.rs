//! The drawing of `stats`: its answer as text, for a person to read with
//! `jq -r .drawing`. It draws what the [`StatsOut`] holds and nothing more, in its
//! order, and reads no clock and no time zone, so the same answer is the same text.
//!
//! Five headed sections, one after another, a blank line between two:
//!
//! ```text
//! Approvals per book
//!   book      approvals  again  revoked  documents
//!   decision          4      1        0          3
//!
//! Approved again most often
//!   again  approvals  document
//!       1          2  decision:DEC-1
//!
//! Pending per book, from a document's identity to its first approval
//!   book      count  median    p90    max
//!   decision      3     13m  2h 5m  2h 5m
//!
//! Runs of judge
//!   count  median  p90  max
//!       2       1    2    2
//!   largest
//!     size  actor  at
//!        2  ann    2026-11-10T03:12:45Z
//!
//! Origins under approvals made again
//!   count  origin
//!       1  decision:DEC-2
//! ```
//!
//! # Lists
//!
//! The documents approved again are those of the answer's list approved again at
//! least once, the first [`AGAIN`] of them (the list is ordered by that count, the
//! most first); the largest runs are the first [`LARGEST`] of the answer's; the
//! origins the first [`ORIGINS`]. A list drawn short says so in its heading, as
//! `Approved again most often (10 of 104)`, `largest (5 of 58)` (of every run) and
//! `Origins under approvals made again (10 of 31)`.
//!
//! # Values
//!
//! A duration is hours and minutes, the seconds left out (`2h 13m`, `13m`), and
//! `< 1m` under a minute either way; a minute or more is after `-` when the identity
//! carries a later time than the approval. A time is RFC 3339 in UTC, to the
//! second; a time it cannot write (before the year 0, or after the last instant
//! `jiff` holds, late in 9999) is `unix ms <millis>`, never a text that looks like a
//! time and is none. A value the answer does not have is `-`. A
//! document is its key, or its identity when it is no valid document now; the
//! documents of no book are under `(no book)`. An artifact is its name, after
//! `<space>:` when its space is not `file`. A section with nothing to count says so
//! in a line of its own.
//!
//! # Columns
//!
//! Widths are display widths (Unicode's East Asian width: a CJK character takes two
//! columns), so a column of names in any script stays aligned. Numbers are aligned
//! to the right of their column, words to the left. A column of words that is not
//! the last is cut to [`NAME`] columns; only the last column is cut to fit the line
//! in [`LINE`] columns. A cut ends in `…` and falls between two characters, which
//! may split a character made of several (an emoji with a modifier, a letter with a
//! combining accent). With these cuts every line is narrower than [`LINE`] columns
//! as long as no number has more than ten digits, more than a ledger holds.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{ChangedOut, DocRef, StatsOut};

/// How many of the documents approved again are drawn.
pub(super) const AGAIN: usize = 10;
/// How many of the largest runs are drawn.
pub(super) const LARGEST: usize = 5;
/// How many origins are drawn.
pub(super) const ORIGINS: usize = 10;
/// The lines are narrower than this, in columns, for every number of ten digits
/// or fewer.
pub(super) const LINE: usize = 100;
/// The widest, in columns, a column of words that is not the last (a book, an
/// actor) is drawn.
const NAME: usize = 40;
/// The earliest time RFC 3339 writes, 0000-01-01T00:00:00Z, in unix milliseconds.
const YEAR_ZERO: i64 = -62_167_219_200_000;

/// Between two columns.
const GAP: &str = "  ";

/// Where a column puts its cells.
#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
}

/// The drawing of an answer of `stats`, one line after another, each ending in a
/// line break.
pub(super) fn draw(stats: &StatsOut) -> String {
    let sections = [
        approvals(stats),
        again(stats),
        pending(stats),
        runs(stats),
        origins(stats),
    ];
    let mut out = String::new();
    for (i, lines) in sections.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn approvals(stats: &StatsOut) -> Vec<String> {
    let mut lines = vec!["Approvals per book".to_owned()];
    let books = &stats.approvals.books;
    if books.iter().all(|b| b.approvals == 0) {
        lines.push("  no approval".to_owned());
        return lines;
    }
    let rows = books
        .iter()
        .map(|b| {
            vec![
                book(b.book.as_deref()),
                b.approvals.to_string(),
                b.again.to_string(),
                b.revoked.to_string(),
                b.docs.to_string(),
            ]
        })
        .collect();
    lines.extend(table(
        "  ",
        &[
            ("book", Align::Left),
            ("approvals", Align::Right),
            ("again", Align::Right),
            ("revoked", Align::Right),
            ("documents", Align::Right),
        ],
        rows,
    ));
    lines
}

fn again(stats: &StatsOut) -> Vec<String> {
    // The list is ordered by `again`, the most first, so those approved again are
    // its first ones.
    let approved_again: Vec<_> = stats
        .approvals
        .docs
        .iter()
        .filter(|d| d.again > 0)
        .collect();
    let mut lines = vec![heading(
        "Approved again most often",
        AGAIN,
        approved_again.len(),
    )];
    if approved_again.is_empty() {
        lines.push("  no document approved again".to_owned());
        return lines;
    }
    let rows = approved_again
        .iter()
        .take(AGAIN)
        .map(|d| vec![d.again.to_string(), d.approvals.to_string(), doc(&d.doc)])
        .collect();
    lines.extend(table(
        "  ",
        &[
            ("again", Align::Right),
            ("approvals", Align::Right),
            ("document", Align::Left),
        ],
        rows,
    ));
    lines
}

fn pending(stats: &StatsOut) -> Vec<String> {
    let mut lines =
        vec!["Pending per book, from a document's identity to its first approval".to_owned()];
    if stats.pending.iter().all(|p| p.count == 0) {
        lines.push("  no document approved".to_owned());
        return lines;
    }
    let rows = stats
        .pending
        .iter()
        .map(|p| {
            vec![
                book(p.book.as_deref()),
                p.count.to_string(),
                optional(p.median_ms, duration),
                optional(p.p90_ms, duration),
                optional(p.max_ms, duration),
            ]
        })
        .collect();
    lines.extend(table(
        "  ",
        &[
            ("book", Align::Left),
            ("count", Align::Right),
            ("median", Align::Right),
            ("p90", Align::Right),
            ("max", Align::Right),
        ],
        rows,
    ));
    lines
}

fn runs(stats: &StatsOut) -> Vec<String> {
    let mut lines = vec!["Runs of judge".to_owned()];
    let runs = &stats.runs;
    if runs.count == 0 {
        lines.push("  no run".to_owned());
        return lines;
    }
    let number = |n: i64| n.to_string();
    lines.extend(table(
        "  ",
        &[
            ("count", Align::Right),
            ("median", Align::Right),
            ("p90", Align::Right),
            ("max", Align::Right),
        ],
        vec![vec![
            runs.count.to_string(),
            optional(runs.median, number),
            optional(runs.p90, number),
            optional(runs.max, number),
        ]],
    ));
    if !runs.largest.is_empty() {
        let drawn = runs.largest.len().min(LARGEST);
        lines.push(format!("  {}", heading("largest", drawn, runs.count)));
        let rows = runs
            .largest
            .iter()
            .take(LARGEST)
            .map(|r| vec![r.size.to_string(), r.actor.clone(), time(r.at)])
            .collect();
        lines.extend(table(
            "    ",
            &[
                ("size", Align::Right),
                ("actor", Align::Left),
                ("at", Align::Left),
            ],
            rows,
        ));
    }
    lines
}

fn origins(stats: &StatsOut) -> Vec<String> {
    let mut lines = vec![heading(
        "Origins under approvals made again",
        ORIGINS,
        stats.origins.len(),
    )];
    if stats.origins.is_empty() {
        lines.push("  no origin under an approval made again".to_owned());
        return lines;
    }
    let rows = stats
        .origins
        .iter()
        .take(ORIGINS)
        .map(|o| {
            let name = match &o.origin {
                ChangedOut::Doc(named) => doc(&named.doc),
                ChangedOut::Artifact(named) if named.space == "file" => named.name.clone(),
                ChangedOut::Artifact(named) => format!("{}:{}", named.space, named.name),
            };
            vec![o.count.to_string(), name]
        })
        .collect();
    lines.extend(table(
        "  ",
        &[("count", Align::Right), ("origin", Align::Left)],
        rows,
    ));
    lines
}

/// A heading, with `(<drawn> of <all>)` after it when fewer are drawn than there are.
fn heading(title: &str, drawn: usize, all: usize) -> String {
    if all > drawn {
        format!("{title} ({drawn} of {all})")
    } else {
        title.to_owned()
    }
}

/// The lines of a table: its head, then a line per row, each after `indent`. A
/// column of words that is not the last is cut to [`NAME`] columns; the last, when
/// it holds words, is cut so that the line stays narrower than [`LINE`] columns.
fn table(indent: &str, head: &[(&str, Align)], mut rows: Vec<Vec<String>>) -> Vec<String> {
    let last = head.len() - 1;
    for row in &mut rows {
        for (i, (cell, (_, align))) in row.iter_mut().zip(head).enumerate() {
            if i < last && matches!(align, Align::Left) {
                *cell = cut(cell, NAME);
            }
        }
    }
    let mut widths: Vec<usize> = head.iter().map(|(word, _)| word.width()).collect();
    for row in &rows {
        for (i, cell) in row.iter().enumerate().take(last) {
            widths[i] = widths[i].max(cell.width());
        }
    }
    let before_last = indent.width() + widths[..last].iter().sum::<usize>() + GAP.len() * last;
    let room = (LINE - 1).saturating_sub(before_last).max(1);
    if matches!(head[last].1, Align::Left) {
        for row in &mut rows {
            row[last] = cut(&row[last], room);
        }
    }
    widths[last] = std::iter::once(head[last].0)
        .chain(rows.iter().map(|row| row[last].as_str()))
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);
    let line = |cells: Vec<&str>| {
        let drawn: Vec<String> = cells
            .iter()
            .zip(head)
            .zip(&widths)
            .map(|((cell, (_, align)), w)| {
                let pad = " ".repeat(w.saturating_sub(cell.width()));
                match align {
                    Align::Left => format!("{cell}{pad}"),
                    Align::Right => format!("{pad}{cell}"),
                }
            })
            .collect();
        format!("{indent}{}", drawn.join(GAP)).trim_end().to_owned()
    };
    let mut lines = vec![line(head.iter().map(|(word, _)| *word).collect())];
    for row in &rows {
        lines.push(line(row.iter().map(String::as_str).collect()));
    }
    lines
}

/// A book, as drawn.
fn book(name: Option<&str>) -> String {
    name.unwrap_or("(no book)").to_owned()
}

/// A document, as drawn: its key, else its identity.
fn doc(doc: &DocRef) -> String {
    doc.key.clone().unwrap_or_else(|| doc.id.clone())
}

/// A value, or `-` when there is none.
fn optional(value: Option<i64>, show: impl Fn(i64) -> String) -> String {
    value.map(show).unwrap_or_else(|| "-".to_owned())
}

/// Milliseconds as hours and minutes: `2h 13m`, `13m`, `< 1m`, the seconds left out.
pub(super) fn duration(millis: i64) -> String {
    let minutes = millis.unsigned_abs() / 60_000;
    if minutes == 0 {
        return "< 1m".to_owned();
    }
    let sign = if millis < 0 { "-" } else { "" };
    let (hours, minutes) = (minutes / 60, minutes % 60);
    if hours == 0 {
        format!("{sign}{minutes}m")
    } else {
        format!("{sign}{hours}h {minutes}m")
    }
}

/// Unix milliseconds as RFC 3339 in UTC, to the second; `unix ms <millis>` when
/// RFC 3339 cannot write that time.
pub(super) fn time(millis: i64) -> String {
    match jiff::Timestamp::from_millisecond(millis) {
        Ok(at) if millis >= YEAR_ZERO => at.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        _ => format!("unix ms {millis}"),
    }
}

/// A text at most `most` columns wide, cut with `…` when it is wider. The cut falls
/// between two characters.
fn cut(text: &str, most: usize) -> String {
    if text.width() <= most {
        return text.to_owned();
    }
    let room = most.saturating_sub(1);
    let mut kept = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > room {
            break;
        }
        used += w;
        kept.push(c);
    }
    kept.push('…');
    kept
}
