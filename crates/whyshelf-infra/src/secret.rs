//! Secret scan: [`SecretScanPort`] by the shapes of well-known secrets.
//!
//! The scan runs before a text is written to the store (a stored object cannot be
//! taken back from a Git history), so it errs on the side of stopping: it looks
//! for what has an unmistakable form, a private key header and the prefixes that
//! cloud and code-hosting services put on their access keys and tokens. It does
//! not guess at high-entropy strings, which would stop ordinary hashes and
//! identifiers, and it does not claim to find every secret: the report names the
//! rule and the line, never the text matched.
//!
//! | rule (as reported) | the shape it looks for |
//! |---|---|
//! | private key header | a `-----BEGIN ... PRIVATE KEY-----` line |
//! | cloud access key id | `AKIA`, `ASIA` and a few sibling prefixes, then 16 capitals or digits |
//! | code-hosting token | `ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_` with 30 or more characters; `github_pat_` with 22 or more |
//! | chat service token | `xox` and a letter, then 10 or more token characters |
//! | cloud API key | `AIza` and 35 characters |
//! | payment live key | `sk_live_` or `rk_live_` with 16 or more |
//! | model API key | `sk-` (or `sk-ant-`) with 32 or more |
//!
//! **What it stops, and when.** Every text about to be stored as an object (the
//! normalized text of a document version) passes through it before anything is
//! written; a hit refuses that write, and ingest then leaves that one file out and
//! reports it, retrying at the next run. **What it does not stop.** A secret that has
//! no such form (a password in a sentence, a high-entropy string, a key of a service
//! not listed) is not seen; the fields of an event (a reason, a hint) and the files
//! of the project outside the documents are not scanned. A document that already
//! holds a secret stays in the working tree and in Git as it is: the scan keeps it
//! out of the store, it does not remove it from the file.

use std::num::NonZeroU32;

use regex::Regex;
use whyshelf_core::app::ports::{SecretHit, SecretScanPort};
use whyshelf_core::app::raw::NormalizedText;
use whyshelf_core::domain::nonempty::NonBlank;

/// A rule: its name, which is reported, and the shape it looks for.
const RULES: &[(&str, &str)] = &[
    (
        "private key header",
        r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----",
    ),
    (
        "cloud access key id",
        r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA|ANVA|AIPA)[0-9A-Z]{16}\b",
    ),
    ("code-hosting token", r"\bgh[pousr]_[A-Za-z0-9]{30,}\b"),
    ("code-hosting token", r"\bgithub_pat_[A-Za-z0-9_]{22,}\b"),
    ("chat service token", r"\bxox[abeoprs]-[A-Za-z0-9-]{10,}\b"),
    ("cloud API key", r"\bAIza[0-9A-Za-z_-]{35}\b"),
    ("payment live key", r"\b[sr]k_live_[0-9A-Za-z]{16,}\b"),
    ("model API key", r"\bsk-(?:ant-)?[A-Za-z0-9_-]{32,}\b"),
];

/// Why the scanner could not be built.
#[derive(Debug, thiserror::Error)]
#[error("the secret pattern `{pattern}` does not compile")]
pub struct SecretError {
    pattern: &'static str,
    #[source]
    source: regex::Error,
}

/// The scanner, with its patterns compiled once.
#[derive(Debug, Clone)]
pub struct RegexSecretScan {
    rules: Vec<(&'static str, Regex)>,
}

impl RegexSecretScan {
    /// Compile the built-in patterns.
    pub fn new() -> Result<Self, SecretError> {
        let rules = RULES
            .iter()
            .map(|&(name, pattern)| {
                Regex::new(pattern)
                    .map(|re| (name, re))
                    .map_err(|source| SecretError { pattern, source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { rules })
    }

    /// The hits in a text: at most one per rule and line, in line order.
    pub fn scan_text(&self, text: &str) -> Vec<SecretHit> {
        let mut hits = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let Some(number) = u32::try_from(index + 1).ok().and_then(NonZeroU32::new) else {
                continue;
            };
            let mut named: Vec<&str> = Vec::new();
            for (name, re) in &self.rules {
                if !named.contains(name) && re.is_match(line) {
                    named.push(name);
                    if let Ok(rule) = NonBlank::new(*name) {
                        hits.push(SecretHit::new(rule, number));
                    }
                }
            }
        }
        hits
    }
}

impl SecretScanPort for RegexSecretScan {
    type Error = std::convert::Infallible;

    async fn scan(&self, text: &NormalizedText) -> Result<Vec<SecretHit>, Self::Error> {
        Ok(self.scan_text(text.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The fixtures are assembled at run time so that this file does not itself hold
    // a string that a secret scanner would report.
    fn fixture(parts: &[&str]) -> String {
        parts.concat()
    }

    fn scanner() -> RegexSecretScan {
        RegexSecretScan::new().unwrap()
    }

    #[test]
    fn a_private_key_header_is_found_with_its_line() {
        let text = fixture(&[
            "notes\n",
            "-----BEGIN ",
            "RSA PRIVATE",
            " KEY-----\n",
            "x\n",
        ]);
        let hits = scanner().scan_text(&text);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rule(), "private key header");
        assert_eq!(hits[0].line(), 2);
    }

    #[test]
    fn an_openssh_and_a_plain_key_header_are_found() {
        for header in ["OPENSSH PRIVATE", "PRIVATE", "ENCRYPTED PRIVATE"] {
            let text = fixture(&["-----BEGIN ", header, " KEY-----"]);
            assert_eq!(scanner().scan_text(&text).len(), 1, "{header}");
        }
    }

    #[test]
    fn cloud_and_hosting_tokens_are_found() {
        let cases = [
            fixture(&["id: AK", "IA", "ABCDEFGHIJKLMNOP", " end"]),
            fixture(&["token gh", "p_", &"a1".repeat(20)]),
            fixture(&["github", "_pat_", &"Ab1_".repeat(8)]),
            fixture(&["xo", "xb-", "1234567890-abcdef"]),
            fixture(&["key=AI", "za", &"x".repeat(35)]),
            fixture(&["sk", "_live_", &"q".repeat(20)]),
            fixture(&["sk", "-ant-", &"z9".repeat(20)]),
        ];
        for text in &cases {
            assert_eq!(scanner().scan_text(text).len(), 1, "{text}");
        }
    }

    #[test]
    fn ordinary_text_is_not_a_hit() {
        let text = "Login takes 8 s.\nsha256:3f1c0000000000000000000000000000000000000000000000000000000000ab\n\
                    uuid 0192e3a4-7b5c-7d3e-8f01-23456789abcd\nan AKIA prefix alone, or sk-short\n\
                    -----BEGIN CERTIFICATE-----\n";
        assert_eq!(scanner().scan_text(text), vec![]);
    }

    #[test]
    fn one_hit_per_rule_and_line_and_lines_are_counted_from_one() {
        let key = fixture(&["AK", "IA", "ABCDEFGHIJKLMNOP"]);
        let text = format!("{key} {key}\nclean\n{key}\n");
        let hits = scanner().scan_text(&text);
        assert_eq!(
            hits.iter().map(SecretHit::line).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[tokio::test]
    async fn the_port_scans_normalized_text() {
        let text = NormalizedText::new(fixture(&["-----BEGIN ", "PRIVATE", " KEY-----\n"]));
        let hits = scanner().scan(&text).await.unwrap();
        assert_eq!(hits.len(), 1);
    }
}
