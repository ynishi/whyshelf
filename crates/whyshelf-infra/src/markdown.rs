//! Markdown documents: text normalization, front matter, sections.
//!
//! # The one normalization rule
//!
//! Every text that is hashed, stored or compared is first normalized by
//! [`normalize`], and the rule is the same everywhere:
//!
//! 1. line breaks become LF (`\r\n` and a lone `\r` are line breaks);
//! 2. the spaces and tabs at the end of each line are removed;
//! 3. the blank lines at the end of the text are collapsed so that the text ends
//!    in exactly one LF (a text with nothing but blank lines becomes empty).
//!
//! There is **no Unicode normalization** (no NFC, no case folding): only white
//! space and line breaks are normalized, so a text keeps the characters its
//! author typed. The `raw` hash of a version is the hash of the normalized text,
//! and the stored object is that text.
//!
//! # Shape of a document
//!
//! A document is a front matter between two `---` lines, then a body made of
//! sections that start with a `## ` heading. [`split_front_matter`] cuts the
//! first from the rest, [`section`] cuts one named section out of the body (the
//! `## Contract` of a Spec, the `## Design Notes` of a Design), and
//! [`conflict_marker`] finds the lines a failed merge leaves behind. A file with
//! a conflict marker is never parsed further: it is recorded as it is and shown
//! as invalid until a person resolves it.

use whyshelf_core::app::raw::NormalizedText;

/// Normalize a text by the rule in the module documentation.
pub fn normalize(text: &str) -> NormalizedText {
    NormalizedText::new(normalize_str(text))
}

fn normalize_str(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(unified.len() + 1);
    for line in unified.split('\n') {
        out.push_str(line.trim_end_matches([' ', '\t']));
        out.push('\n');
    }
    let kept = out.trim_end_matches('\n').len();
    out.truncate(kept);
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// A document cut into its front matter and its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrontMatter<'a> {
    /// The YAML between the two `---` lines, without them.
    pub yaml: &'a str,
    /// Everything after the closing `---` line.
    pub body: &'a str,
}

/// Why a text has no usable front matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrontMatterError {
    /// The text does not start with a `---` line.
    #[error("the text does not start with a `---` line")]
    Missing,
    /// The opening `---` has no closing `---` line.
    #[error("the front matter has no closing `---` line")]
    Unclosed,
}

/// Cut a normalized text into front matter and body.
///
/// The first line must be `---` and a later line must be `---` again.
pub fn split_front_matter(text: &str) -> Result<FrontMatter<'_>, FrontMatterError> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or(FrontMatterError::Missing)?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches('\n') == "---" {
            return Ok(FrontMatter {
                yaml: &rest[..offset],
                body: &rest[offset + line.len()..],
            });
        }
        offset += line.len();
    }
    Err(FrontMatterError::Unclosed)
}

/// The text of the section under `## <heading>`, up to the next `## ` heading,
/// without the blank lines around it. `None` when there is no such heading.
///
/// A line inside a fenced code block is never a heading.
pub fn section<'a>(body: &'a str, heading: &str) -> Option<&'a str> {
    let wanted = format!("## {heading}");
    let mut fenced = false;
    let mut start: Option<usize> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        if bare.starts_with("```") || bare.starts_with("~~~") {
            fenced = !fenced;
        } else if !fenced && bare.starts_with("## ") {
            if let Some(begin) = start {
                return Some(body[begin..offset].trim_matches('\n'));
            }
            if bare.trim_end() == wanted {
                start = Some(offset + line.len());
            }
        }
        offset += line.len();
    }
    start.map(|begin| body[begin..].trim_matches('\n'))
}

/// The text with the front matter cut off and the blank lines at its start
/// removed (what a Finding keeps as its body).
pub fn body_text(body: &str) -> &str {
    body.trim_start_matches('\n')
}

/// Whether the text holds a line that Git leaves for an unresolved merge: one
/// that starts with seven `<` or seven `>` and ends there or goes on after a
/// space.
pub fn conflict_marker(text: &str) -> bool {
    text.lines().any(|line| {
        ["<<<<<<<", ">>>>>>>"].iter().any(|marker| {
            line.strip_prefix(marker)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_breaks_become_lf() {
        assert_eq!(normalize("a\r\nb\rc\n").as_str(), "a\nb\nc\n");
    }

    #[test]
    fn trailing_spaces_and_tabs_of_each_line_are_removed() {
        assert_eq!(normalize("a \t\n  b  \n\tc\t").as_str(), "a\n  b\n\tc\n");
    }

    #[test]
    fn trailing_blank_lines_collapse_to_one_lf() {
        assert_eq!(normalize("a\n\n\n").as_str(), "a\n");
        assert_eq!(normalize("a").as_str(), "a\n");
        assert_eq!(normalize("a\n \n\t\n").as_str(), "a\n");
        // Blank lines inside the text stay.
        assert_eq!(normalize("a\n\nb\n").as_str(), "a\n\nb\n");
    }

    #[test]
    fn an_empty_or_blank_text_is_empty() {
        assert_eq!(normalize("").as_str(), "");
        assert_eq!(normalize(" \n\n").as_str(), "");
    }

    #[test]
    fn there_is_no_unicode_normalization() {
        // "e" + combining acute stays two scalar values; the precomposed form is another text.
        let decomposed = "e\u{0301}\n";
        assert_eq!(normalize(decomposed).as_str(), decomposed);
        assert_ne!(normalize(decomposed), normalize("\u{00e9}\n"));
    }

    #[test]
    fn normalization_is_idempotent() {
        let once = normalize("x  \r\n\r\n y\t\n\n\n");
        assert_eq!(normalize(once.as_str()), once);
    }

    #[test]
    fn front_matter_is_cut_from_the_body() {
        let text = "---\na: 1\nb: 2\n---\n\n## Contract\ntext\n";
        let parts = split_front_matter(text).unwrap();
        assert_eq!(parts.yaml, "a: 1\nb: 2\n");
        assert_eq!(parts.body, "\n## Contract\ntext\n");
    }

    #[test]
    fn missing_and_unclosed_front_matter_are_told_apart() {
        assert_eq!(
            split_front_matter("# Title\n"),
            Err(FrontMatterError::Missing)
        );
        assert_eq!(
            split_front_matter("---\na: 1\n"),
            Err(FrontMatterError::Unclosed)
        );
    }

    #[test]
    fn a_section_runs_to_the_next_heading() {
        let body = "\n## Contract\nline 1\n\nline 2\n\n## Design Notes\nnotes\n";
        assert_eq!(section(body, "Contract"), Some("line 1\n\nline 2"));
        assert_eq!(section(body, "Design Notes"), Some("notes"));
        assert_eq!(section(body, "Other"), None);
    }

    #[test]
    fn a_heading_in_a_code_fence_does_not_end_a_section() {
        let body = "## Contract\n```\n## not a heading\n```\nafter\n";
        assert_eq!(
            section(body, "Contract"),
            Some("```\n## not a heading\n```\nafter")
        );
    }

    #[test]
    fn conflict_markers_are_found() {
        assert!(conflict_marker(
            "a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> topic\n"
        ));
        assert!(conflict_marker("<<<<<<<\n"));
        assert!(!conflict_marker("Title\n=======\n"));
        assert!(!conflict_marker("<<<<<<<<not a marker\n"));
    }
}
