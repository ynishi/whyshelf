//! The JSON form of every answer: one serde type per command, the same
//! type `schema` describes, so the schema and the output cannot drift apart.
//!
//! A report of the use cases holds domain values; this module writes them as data
//! an agent can read without knowing Rust: a document is `{id, key}`, a state is a
//! word, a cause has a `kind` word, the document it names, and a sentence.

use schemars::{JsonSchema, Schema, schema_for};
use serde::Serialize;
use serde_json::Value;
use whyshelf_core::app::shelf::Names;

use super::new_lines::NewLineIn;

/// The answer of `stats`. The measures and the order of every list are defined on
/// the page of `whyshelf_core::domain::shelf::stats`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatsOut {
    /// The `--since` as it was given; null without it.
    pub since: Option<String>,
    /// The approvals, revoked ones included, per document and per book.
    pub approvals: ApprovalsOut,
    /// Per book, the milliseconds from a document's identity to its first approval
    /// that is not revoked.
    pub pending: Vec<BookPendingOut>,
    /// The runs of `judge`: the decisions that share their time and their actor.
    pub runs: RunsOut,
    /// The documents and artifacts whose pinned hash differed under an approval made
    /// again from the approval before it, the most counted first.
    pub origins: Vec<OriginCountOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
    /// The same measures as text for a person (`jq -r .drawing`), made from this
    /// answer alone, so the same answer is the same text.
    pub drawing: String,
}

/// The approvals, per document and per book.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovalsOut {
    /// Each document with an approval, the most approved again first, then by key.
    pub docs: Vec<DocApprovalsOut>,
    /// Each book in the order of the declaration, then (`book` null) the documents
    /// that are no valid document now.
    pub books: Vec<BookApprovalsOut>,
}

/// The approvals of one document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocApprovalsOut {
    /// The document.
    pub doc: DocRef,
    /// Its approvals, revoked or not.
    pub approvals: usize,
    /// Those after an earlier approval of it that is not revoked.
    pub again: usize,
    /// Those revoked.
    pub revoked: usize,
}

/// The approvals of the documents of one book.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BookApprovalsOut {
    /// The book; null for the documents that are no valid document now.
    pub book: Option<String>,
    /// Its documents with an approval.
    pub docs: usize,
    /// Their approvals, revoked or not.
    pub approvals: usize,
    /// Those after an earlier approval of the same document that is not revoked.
    pub again: usize,
    /// Those revoked.
    pub revoked: usize,
}

/// The pending times of one book.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BookPendingOut {
    /// The book; null for the documents that are no valid document now.
    pub book: Option<String>,
    /// How many documents were approved.
    pub count: usize,
    /// The median, nearest-rank, in milliseconds; null when none.
    pub median_ms: Option<i64>,
    /// The 90th percentile, nearest-rank, in milliseconds; null when none.
    pub p90_ms: Option<i64>,
    /// The longest, in milliseconds; null when none.
    pub max_ms: Option<i64>,
}

/// The runs of `judge`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RunsOut {
    /// How many runs.
    pub count: usize,
    /// The median number of decisions of a run, nearest-rank; null when none.
    pub median: Option<i64>,
    /// The 90th percentile of the same, nearest-rank; null when none.
    pub p90: Option<i64>,
    /// The most decisions of a run; null when none.
    pub max: Option<i64>,
    /// The largest runs, the largest first, then the earliest.
    pub largest: Vec<RunOut>,
}

/// One run.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RunOut {
    /// When (unix milliseconds).
    pub at: i64,
    /// Who.
    pub actor: String,
    /// How many decisions.
    pub size: usize,
}

/// A document or an artifact that changed under approvals made again.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OriginCountOut {
    /// What it is.
    pub origin: ChangedOut,
    /// Under how many approvals made again.
    pub count: usize,
}

/// What an origin over time is: a document or an artifact, written as `stale`'s
/// origins write them.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangedOut {
    /// A document (its key is null when no valid document has it now).
    Doc(DocNamed),
    /// An artifact.
    Artifact(ArtifactNamed),
}

/// The JSON Schema of every answer, and of what a command reads.
#[derive(Debug, Serialize)]
pub struct SchemasOut {
    /// The version of the answers these schemas describe.
    pub version: u32,
    /// By command.
    pub outputs: serde_json::Map<String, Value>,
    /// What a command reads, by command: the shape of one line of the file of
    /// `new --from`.
    pub inputs: serde_json::Map<String, Value>,
}

/// The JSON Schema of every answer, by command.
pub fn schemas() -> SchemasOut {
    let mut outputs = serde_json::Map::new();
    let mut put = |name: &str, schema: Schema| {
        outputs.insert(name.to_owned(), versioned_schema(schema.to_value()));
    };
    put("check", schema_for!(BooksCheckOut));
    put("map", schema_for!(MapOut));
    put("status", schema_for!(StatusOut));
    put("get", schema_for!(GetOut));
    put("get --approved", schema_for!(ApprovedOut));
    put("trace", schema_for!(TraceOut));
    put("unlinked", schema_for!(UnlinkedOut));
    put("stale", schema_for!(StaleOut));
    put("stats", schema_for!(StatsOut));
    put("moves", schema_for!(MovesOut));
    put("diff", schema_for!(DiffOut));
    put("judge", schema_for!(JudgedAllOut));
    put("judge approve --stale", schema_for!(ChosenApprovedOut));
    put("judge approve --pending", schema_for!(ChosenApprovedOut));
    put("judge same", schema_for!(DeclaredAllOut));
    put("judge moved", schema_for!(MovedAllOut));
    put("judge moved --prefix", schema_for!(MovedPrefixOut));
    put("judge exempt", schema_for!(ExemptedAllOut));
    put("judge withdraw", schema_for!(WithdrawnOut));
    put("new", schema_for!(CreatedOut));
    put("new --from", schema_for!(NewManyOut));
    put("init", schema_for!(InitOut));
    put("fmt", schema_for!(FmtOut));
    put("error", schema_for!(ErrorOut));
    let mut inputs = serde_json::Map::new();
    inputs.insert("new --from".to_owned(), schema_for!(NewLineIn).to_value());
    SchemasOut {
        version: FORMAT_VERSION,
        outputs,
        inputs,
    }
}

/// The version of the JSON every command prints, as the `version` field of each
/// answer.
///
/// Why: a reader (often an agent) can tell which shape it has. Changes within a
/// version only add: a new field, a new word in a field that holds words. Anything
/// else is a new version.
pub const FORMAT_VERSION: u32 = 1;

/// An answer with its `version` field, the one place every printed answer passes.
pub fn versioned(answer: Value) -> Value {
    match answer {
        Value::Object(mut fields) => {
            fields.insert("version".to_owned(), Value::from(FORMAT_VERSION));
            Value::Object(fields)
        }
        other => other,
    }
}

/// A schema of an answer, with the `version` field `versioned` adds.
fn versioned_schema(mut schema: Value) -> Value {
    if let Some(Value::Object(properties)) = schema.get_mut("properties") {
        properties.insert(
            "version".to_owned(),
            serde_json::json!({ "type": "integer", "const": FORMAT_VERSION }),
        );
    }
    if let Some(Value::Array(required)) = schema.get_mut("required") {
        required.push(Value::from("version"));
    }
    schema
}

/// Writes identities as `{id, key}` with the keys of one report.
struct Namer<'a>(&'a Names);

/// The root `init` or `stale` ran with, as the `next` commands are to carry it.
#[derive(Debug, Clone, Copy)]
pub enum NextRoot<'a> {
    /// No `--root`: the commands carry none.
    Current,
    /// `--root` as typed: each command carries `--root=<it>`, one shell word.
    Given(&'a str),
    /// A `--root` that is no UTF-8 text: no command can be written with it, so
    /// none is given (the command line says so on stderr).
    NotText,
}

mod cause;
mod check;
mod diagram;
mod doc;
mod error;
mod fmt;
mod get;
mod init;
mod judge;
mod map;
mod moves;
mod named;
mod new;
mod next;
mod snapshot;
mod stale;
mod stats;
mod status;
mod summary;
mod trace;
mod unlinked;
mod untold;
mod words;

pub use cause::{
    AbsentOut, CauseOut, ExemptOut, MovedNameOut, NoticeOut, RepeatedOut, UnknownOut, UnneededOut,
};
pub use check::{BooksCheckOut, books_check, check_notes, check_warnings};
pub use doc::{HealthOut, LinkOut, LinkTargetOut, RootOut, RowOut};
pub use error::{ErrorOut, error, failure, lines_refused, usage};
pub use fmt::{FmtOut, fmt, fmt_warnings};
pub use get::{GetOut, get};
pub use init::{InitOut, initialized};
pub use judge::{
    ChosenApprovedOut, DeclaredAllOut, ExemptedAllOut, JudgedAllOut, MovedAllOut, MovedPrefixOut,
    WithdrawnOut, chosen_approved, chosen_approved_warnings, exempted, judged, judged_warnings,
    moved, moved_prefix, same_meaning, withdrawn,
};
pub use map::{MapOut, map};
pub use moves::{MovesOut, moves};
pub use named::{ArtifactNamed, DocNamed, DocRef};
pub use new::{CreatedOut, NewManyOut, created, new_many};
pub use snapshot::{ApprovedOut, DiffOut, approved, diff};
pub use stale::{StaleOut, stale, stale_warnings};
pub use stats::stats;
pub use status::{StatusOut, status};
pub use trace::{TraceOut, trace};
pub use unlinked::{UnlinkedOut, unlinked};
use words::shell_word;
pub use words::standing;

#[cfg(test)]
use doc::artifact_target;
#[cfg(test)]
use map::KeyGrammarOut;
#[cfg(test)]
use snapshot::{DiffItemOut, diff_item, retrieval};
#[cfg(test)]
use whyshelf_core::app::shelf::{MapReport, Unknown};
#[cfg(test)]
use whyshelf_core::domain::shelf::freshness::{Cause, Freshness, Standing};
#[cfg(test)]
use whyshelf_core::domain::shelf::ledger::Record;
#[cfg(test)]
use whyshelf_core::domain::shelf::schema::{BookName, KeyPattern};
#[cfg(test)]
use whyshelf_core::domain::shelf::snapshot::{ItemNow, ItemRole, Retrieval};

#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod cover_tests;
#[cfg(test)]
mod map_tests;
#[cfg(test)]
mod same_meaning_tests;
#[cfg(test)]
mod stats_tests;
#[cfg(test)]
mod untold_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_covers_every_answer_with_its_version() {
        let schemas = schemas();
        assert_eq!(schemas.version, FORMAT_VERSION);
        let outputs = schemas.outputs;
        for command in [
            "check",
            "map",
            "status",
            "get",
            "get --approved",
            "trace",
            "stale",
            "stats",
            "diff",
            "judge",
            "judge approve --stale",
            "judge approve --pending",
            "judge same",
            "judge withdraw",
            "new",
            "init",
            "fmt",
            "error",
        ] {
            assert!(outputs.contains_key(command), "{command}");
            let schema = serde_json::to_string(&outputs[command]).unwrap();
            assert!(schema.contains("\"version\""), "{command} has no version");
        }
        let new = serde_json::to_string(&outputs["new"]).unwrap();
        for field in ["doc", "file", "unresolved_links", "written", "why"] {
            assert!(new.contains(&format!("\"{field}\"")), "{field}");
        }
        // The rule of a run is one of two words, in `judged` and in `left_out`.
        let chosen = serde_json::to_string(&outputs["judge approve --pending"]).unwrap();
        for word in ["\"const\":\"stale\"", "\"const\":\"pending\"", "\"rule\""] {
            assert!(chosen.contains(word), "{word}");
        }
        let init = serde_json::to_string(&outputs["init"]).unwrap();
        for field in ["declaration", "documents", "key", "path", "next"] {
            assert!(init.contains(&format!("\"{field}\"")), "{field}");
        }
        let fmt = serde_json::to_string(&outputs["fmt"]).unwrap();
        for field in [
            "rewritten",
            "in_form",
            "not_in_form",
            "left_alone",
            "reason",
        ] {
            assert!(fmt.contains(&format!("\"{field}\"")), "{field}");
        }
        let judge = serde_json::to_string(&outputs["judge"]).unwrap();
        for field in ["judged", "left_out", "reason", "approval"] {
            assert!(judge.contains(&format!("\"{field}\"")), "{field}");
        }
        let error = serde_json::to_string(&outputs["error"]).unwrap();
        assert!(error.contains("\"problems\""));
        let diff = serde_json::to_string(&outputs["diff"]).unwrap();
        for field in ["approved_hash", "current_hash", "retrieval", "diff"] {
            assert!(diff.contains(field), "{field}");
        }
        let stats = serde_json::to_string(&outputs["stats"]).unwrap();
        for field in [
            "since",
            "approvals",
            "again",
            "revoked",
            "pending",
            "median_ms",
            "p90_ms",
            "runs",
            "largest",
            "origins",
            "health",
            "drawing",
        ] {
            assert!(stats.contains(&format!("\"{field}\"")), "{field}");
        }
        // A cause names the artifact it is about, beside the fields it had.
        let stale = serde_json::to_string(&outputs["stale"]).unwrap();
        for field in ["kind", "severity", "upstream", "link", "detail", "artifact"] {
            assert!(stale.contains(&format!("\"{field}\"")), "{field}");
        }
    }

    #[test]
    fn a_root_is_one_shell_word_in_the_next_commands() {
        assert_eq!(shell_word("../my-project"), "../my-project");
        assert_eq!(shell_word("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word(""), "''");
    }

    #[test]
    fn a_retrieval_shows_text_only_when_it_was_read_as_utf8() {
        let read = retrieval(&Retrieval::Read(b"text\n".to_vec()));
        assert_eq!(
            (read.state, read.text.as_deref(), read.bytes),
            ("read", Some("text\n"), Some(5))
        );
        let binary = retrieval(&Retrieval::Read(vec![0xff, 0xfe]));
        assert_eq!((binary.text, binary.bytes), (None, Some(2)));
        let none = retrieval(&Retrieval::NoAddress);
        assert_eq!((none.state, none.bytes), ("no_address", None));
    }
}
