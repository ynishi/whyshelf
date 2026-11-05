//! The output of every command, pinned: JSON, Markdown and table.
//!
//! One scripted project (the same slow-login story as the end-to-end test) is run
//! command by command, and what each command printed is compared with
//! `tests/golden/outputs.txt`. What changes from run to run (identifiers, times,
//! hashes, the temporary directory) is replaced by a placeholder first, so the
//! file holds the shape and the words of every answer and nothing else.
//!
//! The file is the contract of `--format json` (keys and shapes), and of the two
//! text formats. To accept a change on purpose, run the test with
//! `WHYSHELF_UPDATE_GOLDEN=1` and read the diff.

mod common;

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::PathBuf;

use common::{Repo, finding_text};
use serde_json::Value;

const SPEC: &str = "---\ngrounds:\n  - \"finding/login-feels-slow\"\naccept:\n  pins:\n    - id: PIN-LOGIN-CACHE\n      contract: \"The session is reused for five minutes\"\n  hyp:\n    - id: HYP-FASTER-LOGIN\n      metric: \"median login time\"\n      target: \"under 2 s\"\n---\n\n## Contract\nThe session is reused for five minutes.\n";

const DESIGN: &str = "---\nspec: \"spec/session-reuse\"\ncrux:\n  question: \"Where is the session kept?\"\n  options:\n    - id: opt-memory\n      summary: \"Keep it in memory\"\n      chosen: true\n      reason: \"No new storage to secure\"\n    - id: opt-disk\n      summary: \"Keep it on disk\"\n      rejected: \"A file to protect and to clean up\"\nitems:\n  - number: 1\n    summary: \"Reuse the session inside the window\"\n    pins: [PIN-LOGIN-CACHE]\n---\n\n## Design Notes\nKeep it simple.\n";

const TEST_FILE: &str = "// @pin: PIN-LOGIN-CACHE\n#[test]\nfn the_session_is_reused() {}\n";

fn is_hex(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

/// How many bytes from `at` match `pattern`, where `d` is a digit, `x` a hex digit
/// and any other byte stands for itself; `None` when they do not.
fn matches_at(bytes: &[u8], at: usize, pattern: &str) -> Option<usize> {
    let pattern = pattern.as_bytes();
    let window = bytes.get(at..at + pattern.len())?;
    window
        .iter()
        .zip(pattern)
        .all(|(&b, &p)| match p {
            b'd' => b.is_ascii_digit(),
            b'x' => is_hex(b),
            other => b == other,
        })
        .then_some(pattern.len())
}

/// The length of a run of hex digits starting at `at`.
fn hex_run(bytes: &[u8], at: usize) -> usize {
    bytes[at..].iter().take_while(|&&b| is_hex(b)).count()
}

/// Replace what changes from run to run by a placeholder.
fn normalize(text: &str, roots: &[String], table: bool) -> String {
    let mut text = text.to_owned();
    for root in roots {
        text = text.replace(root.as_str(), "<root>");
    }
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        // A time: 2026-11-04T21:40:00, a fraction, and the offset.
        if let Some(n) = matches_at(bytes, i, "dddd-dd-ddTdd:dd:dd") {
            let mut end = i + n;
            if bytes.get(end) == Some(&b'.') {
                end += 1;
                while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                }
            }
            if bytes.get(end) == Some(&b'Z') {
                end += 1;
            } else if let Some(n) =
                matches_at(bytes, end, "+dd:dd").or_else(|| matches_at(bytes, end, "-dd:dd"))
            {
                end += n;
            }
            out.push_str("<time>");
            i = end;
            continue;
        }
        if boundary {
            if let Some(n) = matches_at(bytes, i, "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx") {
                out.push_str("<uuid>");
                i += n;
                continue;
            }
            let rest = &text[i..];
            let (prefix, at) = if rest.starts_with("sha256:") {
                ("sha256:", i + 7)
            } else {
                ("", i)
            };
            let run = hex_run(bytes, at);
            let after = bytes.get(at + run).copied();
            let word_ends = !after.is_some_and(|b| b.is_ascii_alphanumeric());
            if run == 64 && word_ends {
                out.push_str("<hash>");
                i = at + run;
                continue;
            }
            if run == 40 && prefix.is_empty() && word_ends {
                out.push_str("<revision>");
                i = at + run;
                continue;
            }
        }
        let ch = text[i..].chars().next().expect("a character");
        out.push(ch);
        i += ch.len_utf8();
    }
    if table {
        // The columns are as wide as their widest cell, and a placeholder is not as
        // wide as the value it stands for: only the gaps are compared.
        out = out
            .lines()
            .map(|line| {
                let mut collapsed = String::new();
                let mut spaces = 0;
                for ch in line.chars() {
                    if ch == ' ' {
                        spaces += 1;
                        if spaces <= 2 {
                            collapsed.push(ch);
                        }
                    } else {
                        spaces = 0;
                        collapsed.push(ch);
                    }
                }
                collapsed
            })
            .collect::<Vec<_>>()
            .join("\n");
        out.push('\n');
    }
    out
}

struct Golden {
    repo: Repo,
    roots: Vec<String>,
    text: String,
}

impl Golden {
    /// Run a command, once per format asked for, and put what it printed in the file.
    async fn show(&mut self, formats: &[&str], args: &[&str]) {
        for format in formats {
            // The format goes first: `tag` takes anything after it as a change.
            let mut all = vec!["--format", format];
            all.extend(args);
            let out = self.repo.run(&all).await;
            let shown = normalize(out.stdout.trim_end(), &self.roots, *format == "table");
            let _ = writeln!(
                self.text,
                "### whyshelf {} (exit {})\n{}\n",
                normalize(&all.join(" "), &self.roots, false),
                out.code.map_or("none".to_owned(), |c| c.to_string()),
                shown.trim_end()
            );
        }
    }

    /// Every format.
    async fn all(&mut self, args: &[&str]) {
        self.show(&["json", "markdown", "table"], args).await;
    }

    /// A command that changes something runs once.
    async fn once(&mut self, format: &str, args: &[&str]) {
        self.show(&[format], args).await;
    }
}

fn type_matches(name: &str, value: &Value) -> bool {
    match name {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
    }
}

/// Check `value` against `schema` (the part of JSON Schema the generated one uses:
/// `$ref`, `type`, `properties`, `required`, `items`, `allOf`, `anyOf`, `oneOf`,
/// `const`, `enum`). Returns the keys the schema declares for an object, so that
/// [`check_object`] can refuse a key nothing declares.
fn check(root: &Value, schema: &Value, value: &Value) -> Result<BTreeSet<String>, String> {
    let mut known = BTreeSet::new();
    let Value::Object(schema) = schema else {
        return match schema {
            Value::Bool(true) => Ok(known),
            other => Err(format!("a schema that is {other}")),
        };
    };
    if let Some(Value::String(reference)) = schema.get("$ref") {
        let name = reference
            .strip_prefix("#/$defs/")
            .ok_or_else(|| format!("a reference {reference}"))?;
        return check(root, &root["$defs"][name], value);
    }
    match schema.get("type") {
        Some(Value::String(name)) if !type_matches(name, value) => {
            return Err(format!("{value} is not a {name}"));
        }
        Some(Value::Array(names))
            if !names
                .iter()
                .any(|n| type_matches(n.as_str().unwrap_or(""), value)) =>
        {
            return Err(format!("{value} is none of {names:?}"));
        }
        _ => {}
    }
    if let Some(expected) = schema.get("const")
        && expected != value
    {
        return Err(format!("{value} is not {expected}"));
    }
    if let Some(Value::Array(options)) = schema.get("enum")
        && !options.contains(value)
    {
        return Err(format!("{value} is none of {options:?}"));
    }
    if let (Some(Value::Object(properties)), Value::Object(object)) =
        (schema.get("properties"), value)
    {
        for (key, sub) in properties {
            known.insert(key.clone());
            if let Some(inner) = object.get(key) {
                check_object(root, sub, inner).map_err(|e| format!("{key}: {e}"))?;
            }
        }
    }
    if let (Some(Value::Array(required)), Value::Object(object)) = (schema.get("required"), value) {
        for key in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(key) {
                return Err(format!("`{key}` is missing"));
            }
        }
    }
    if let (Some(items), Value::Array(list)) = (schema.get("items"), value) {
        for (n, item) in list.iter().enumerate() {
            check_object(root, items, item).map_err(|e| format!("[{n}]: {e}"))?;
        }
    }
    if let Some(Value::Array(all)) = schema.get("allOf") {
        for sub in all {
            known.extend(check(root, sub, value)?);
        }
    }
    for either in ["anyOf", "oneOf"] {
        if let Some(Value::Array(options)) = schema.get(either) {
            let mut errors = Vec::new();
            let mut matched = false;
            for sub in options {
                match check(root, sub, value) {
                    Ok(keys) => {
                        matched = true;
                        known.extend(keys);
                    }
                    Err(error) => errors.push(error),
                }
            }
            if !matched {
                return Err(format!("none of the alternatives fits: {errors:?}"));
            }
        }
    }
    Ok(known)
}

/// [`check`], and no key that the schema does not declare.
fn check_object(root: &Value, schema: &Value, value: &Value) -> Result<(), String> {
    let known = check(root, schema, value)?;
    if let Value::Object(object) = value
        && !known.is_empty()
        && let Some(key) = object.keys().find(|key| !known.contains(*key))
    {
        return Err(format!("`{key}` is declared nowhere in {known:?}"));
    }
    Ok(())
}

/// The JSON answers of the golden file: the command line and the text.
fn golden_json_answers(golden: &str) -> Vec<(String, String)> {
    golden
        .split("### ")
        .skip(1)
        .filter_map(|chunk| {
            let (header, body) = chunk.split_once('\n')?;
            (header.contains("--format json")
                && header.ends_with("(exit 0)")
                && body.trim_start().starts_with('{'))
            .then(|| (header.to_owned(), body.trim().to_owned()))
        })
        .collect()
}

#[tokio::test]
async fn the_json_of_every_command_fits_the_schema_that_is_made_from_its_types() {
    let repo = Repo::new().await;
    let schema = repo.run(&["schema", "output"]).await.ok().json();
    let golden = tokio::fs::read_to_string(golden_path())
        .await
        .expect("the golden file");
    let answers = golden_json_answers(&golden);
    assert!(answers.len() > 30, "{} answers", answers.len());
    let mut seen = BTreeSet::new();
    for (header, text) in &answers {
        let value: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{header}: {e}"));
        if let Err(error) = check_object(&schema, &schema, &value) {
            panic!("{header}: {error}\n{text}");
        }
        if let Value::Object(object) = &value {
            seen.extend(object.keys().cloned());
        }
    }
    // The golden file shows every kind of answer, so every one is checked.
    for key in [
        "context",
        "created",
        "registered",
        "recorded",
        "check",
        "approved",
        "entity",
        "unresolved",
        "external",
        "found",
        "entities",
        "nodes",
        "events",
        "approvals",
        "rebuilt",
        "versions",
    ] {
        assert!(
            seen.contains(key),
            "no answer with `{key}` in the golden file"
        );
    }
}

/// A schema that is not about the answers would let anything through: it refuses
/// an answer of a shape no command has.
#[tokio::test]
async fn the_schema_refuses_an_answer_that_no_command_gives() {
    let repo = Repo::new().await;
    let schema = repo.run(&["schema", "output"]).await.ok().json();
    for bad in [
        r#"{"entities": [{"id": 1}]}"#,
        r#"{"status": []}"#,
        r#"{"rebuilt": {"revised": "many"}}"#,
        r#"{"found": [], "page": {"total": 0}, "surprise": true}"#,
    ] {
        let value: Value = serde_json::from_str(bad).expect("JSON");
        assert!(check_object(&schema, &schema, &value).is_err(), "{bad}");
    }
}

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/outputs.txt")
}

#[tokio::test]
async fn every_command_prints_what_the_golden_file_says() {
    let repo = Repo::new().await;
    repo.init_project().await;
    let root = repo.root().to_string_lossy().into_owned();
    let mut roots = vec![root.clone()];
    if let Ok(real) = tokio::fs::canonicalize(repo.root()).await {
        roots.push(real.to_string_lossy().into_owned());
    }
    roots.sort_by_key(|r| std::cmp::Reverse(r.len()));
    let mut g = Golden {
        repo,
        roots,
        text: String::new(),
    };

    // Starting documents: a Finding made from a template, one written by hand.
    g.once(
        "json",
        &[
            "finding",
            "new",
            "--type",
            "usage_observation",
            "--title",
            "Login feels slow",
        ],
    )
    .await;
    g.repo
        .write(
            "docs/finding/login-feels-slow.md",
            &finding_text("Login feels slow", "It takes eight seconds."),
        )
        .await;
    g.repo
        .write(
            "docs/finding/other.md",
            &finding_text("Other", "Something else."),
        )
        .await;
    g.once(
        "json",
        &["add", "docs/finding/other.md", "--alias", "finding/other"],
    )
    .await;
    g.once("markdown", &["add", "docs"]).await;
    g.once("table", &["add", "docs"]).await;

    // CR, then a Spec that is only a template: the gate refuses it.
    g.all(&["check", "cr", "finding/login-feels-slow"]).await;
    g.once(
        "json",
        &["check", "cr", "finding/login-feels-slow", "--approve"],
    )
    .await;
    g.once(
        "markdown",
        &[
            "spec",
            "new",
            "spec/session-reuse",
            "--from",
            "finding/login-feels-slow",
        ],
    )
    .await;
    g.all(&["check", "cs", "spec/session-reuse"]).await;
    g.repo.write("docs/spec/session-reuse.md", SPEC).await;
    g.once("json", &["check", "cs", "spec/session-reuse", "--approve"])
        .await;
    // The approvals of an entity are listed by their identifiers, which are
    // hashes: the Spec is looked at while it has one.
    g.all(&["get", "spec/session-reuse"]).await;

    // CI.
    g.once(
        "table",
        &[
            "design",
            "new",
            "design/session-reuse",
            "--spec",
            "spec/session-reuse",
        ],
    )
    .await;
    g.repo.write("docs/design/session-reuse.md", DESIGN).await;
    g.once(
        "json",
        &["check", "ci", "design/session-reuse", "--approve"],
    )
    .await;

    // CA at a committed revision, and CM.
    g.repo.write("tests/session.rs", TEST_FILE).await;
    g.repo.commit_all("Implement").await;
    g.all(&["check", "ca", "design/session-reuse#1"]).await;
    g.once(
        "json",
        &["check", "ca", "design/session-reuse#1", "--approve"],
    )
    .await;
    // Only one measurement: records of one gate are listed by their identifiers,
    // which are hashes, and the file must not depend on them.
    g.once(
        "markdown",
        &[
            "check",
            "cm",
            "spec/session-reuse",
            "fail-wiring",
            "--item-hint",
            "the cache is not wired",
        ],
    )
    .await;
    g.once("json", &["tag", "finding/other", "+area:auth"])
        .await;

    // The views.
    g.all(&["context"]).await;
    g.all(&["status"]).await;
    for target in [
        "finding/login-feels-slow",
        "design/session-reuse",
        "design/session-reuse#1",
        "https://example.invalid/page",
        "private:01912345-6789-7abc-8def-0123456789ab",
    ] {
        g.all(&["get", target]).await;
    }
    g.all(&["find", "kind:finding"]).await;
    g.all(&["find", "kind:finding", "--limit", "1"]).await;
    g.all(&["find", "kind:spec", "--fields", "title,tags,state"])
        .await;
    g.all(&["trace", "spec/session-reuse"]).await;
    g.all(&["changelog", "finding/other"]).await;
    g.all(&["audit"]).await;
    g.all(&["audit", "kind:spec"]).await;
    g.all(&["migrate"]).await;

    // A withdrawn approval and a moved file show in the history and the audit.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let audit = g.repo.json(&["audit"]).await;
    let ca_event = common::row_where(common::rows(&audit, "approvals"), "gate", "CA")["event"]
        .as_str()
        .expect("an event")
        .to_owned();
    g.once(
        "json",
        &["revoke", &ca_event, "--reason", "the tests were too thin"],
    )
    .await;
    tokio::fs::rename(
        g.repo.root().join("docs/finding/other.md"),
        g.repo.root().join("docs/finding/moved.md"),
    )
    .await
    .expect("a file can be moved");
    g.once("json", &["mv", "finding/other", "docs/finding/moved.md"])
        .await;
    g.all(&["changelog", "finding/other"]).await;
    g.all(&["changelog", "spec/session-reuse"]).await;
    g.all(&["changelog", "design/session-reuse#1"]).await;
    g.all(&["audit"]).await;

    // What the status has to say when something is off: a file gone, a file
    // changed since it was registered, a file nobody registered.
    g.repo
        .write("docs/finding/new.md", &finding_text("New", "x"))
        .await;
    g.repo
        .write(
            "docs/finding/moved.md",
            &finding_text("Other", "Something else, edited."),
        )
        .await;
    g.repo.write("docs/finding/login-feels-slow.md", "").await;
    tokio::fs::remove_file(g.repo.root().join("docs/design/session-reuse.md"))
        .await
        .expect("a file can be removed");
    g.all(&["status"]).await;
    g.all(&["get", "design/session-reuse"]).await;
    // A file that cannot be read.
    g.all(&["get", "finding/login-feels-slow"]).await;
    g.once("json", &["rebuild"]).await;
    g.once("table", &["rebuild"]).await;

    let path = golden_path();
    if std::env::var_os("WHYSHELF_UPDATE_GOLDEN").is_some() {
        tokio::fs::create_dir_all(path.parent().expect("a directory"))
            .await
            .expect("a directory");
        tokio::fs::write(&path, &g.text)
            .await
            .expect("the golden file can be written");
        return;
    }
    let want = tokio::fs::read_to_string(&path).await.unwrap_or_else(|e| {
        panic!(
            "cannot read {} ({e}); run with WHYSHELF_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    if want != g.text {
        let first = want
            .lines()
            .zip(g.text.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| want.lines().count().min(g.text.lines().count()));
        let context: Vec<&str> = g
            .text
            .lines()
            .skip(first.saturating_sub(3))
            .take(8)
            .collect();
        let expected: Vec<&str> = want.lines().skip(first.saturating_sub(3)).take(8).collect();
        panic!(
            "the output differs from {} at line {}\n--- expected\n{}\n--- got\n{}",
            path.display(),
            first + 1,
            expected.join("\n"),
            context.join("\n")
        );
    }
}
