//! The commands, as clap types.

use clap::{Args, Subcommand};
use whyshelf_core::domain::shelf::artifact::ArtifactName;
use whyshelf_core::domain::shelf::lookup::Reference;

/// The commands on the documents whose kinds the project declares in
/// `.whyshelf/books.toml`.
#[derive(Debug, Subcommand)]
pub enum BooksCommand {
    /// Check the declaration, the documents and the ledger; exit 1 if anything is
    /// wrong.
    Check,
    /// What the declaration says: every book and every link, with a drawing of
    /// them; it reads nothing else, and exits 1 as `check` does when the
    /// declaration cannot be used.
    Map,
    /// Every document, what was decided about it and whether it still holds.
    Status,
    /// One document in full, with where its approval stops holding.
    Get {
        /// A key (`S-1`, `spec:S-1`), an identifier or a short identifier.
        reference: Reference,
        /// Show what the approval that stands saw instead: the document, the
        /// upstreams and the artifacts, each with its content as it was approved, or
        /// why that cannot be read back.
        #[arg(long)]
        approved: bool,
    },
    /// What a document links to and what links to it, transitively.
    Trace {
        /// A key, an identifier or a short identifier.
        reference: Reference,
    },
    /// The documents linked neither way, those no link reaches and those that hold
    /// none, those decided against apart; exit 0 whenever the declaration can be
    /// used, whatever it lists.
    Unlinked {
        /// Only the documents of this book.
        #[arg(long)]
        book: Option<String>,
        /// Of the two one-way lists, only `no_incoming`.
        #[arg(long, conflicts_with = "no_outgoing")]
        no_incoming: bool,
        /// Of the two one-way lists, only `no_outgoing`.
        #[arg(long)]
        no_outgoing: bool,
    },
    /// The approvals that no longer hold, with where each starts to fail.
    Stale {
        /// Exit 1 when any approval does not hold (as `git diff --exit-code`), so a
        /// script or CI can fail on it; without it the exit is 0.
        #[arg(long)]
        exit_code: bool,
    },
    /// Each item of the approval that stands about a document, as it was approved and
    /// as it is now, with a line diff where both are text.
    Diff {
        /// A key, an identifier or a short identifier.
        reference: Reference,
    },
    /// Record a judgement about a document (it and `new` are the only ones of these
    /// that write).
    #[command(subcommand)]
    Judge(Verb),
    /// Create one new document, valid for its book, at `<book dir>/<key>.md` (never
    /// in place of a file that is there).
    New {
        /// The book.
        book: String,
        /// A shorthand for the entry `title=<text>`, for a book that declares a
        /// `title` field; a book declares whether a title is required.
        #[arg(long)]
        title: Option<String>,
        /// The key (default: the book's next number; a book of slugs needs one).
        #[arg(long)]
        key: Option<String>,
        /// More entries, `<entry>=<value>`. A list field or a link takes `a,b`, or one
        /// item at a time with `<entry>+=<item>`, commas kept; a truth value `true` or
        /// `false`; a whole number its digits. Links are keys.
        entries: Vec<String>,
        /// The body.
        #[arg(long)]
        body: Option<String>,
    },
    /// The JSON Schema of the answer of every command.
    Schema,
}

/// What every verb takes.
#[derive(Debug, Args)]
pub struct Common {
    /// The documents, in order: keys, identifiers or short identifiers. They are
    /// judged one after another on one reading of the project, upstream first as
    /// written, and written all together, or none when one is refused.
    #[arg(required = true, num_args = 1..)]
    pub references: Vec<Reference>,
    /// Why.
    #[arg(long)]
    pub reason: Option<String>,
    /// Who judges (default: `WHYSHELF_ACTOR`, else Git's `user.name`).
    #[arg(long)]
    pub actor: Option<String>,
}

/// The verbs of a judgement.
#[derive(Debug, Subcommand)]
pub enum Verb {
    /// The document is sound as it is now.
    Approve {
        /// The document and why.
        #[command(flatten)]
        common: Common,
        /// When the approval lapses (RFC 3339, or a date).
        #[arg(long)]
        until: Option<String>,
    },
    /// The document is refused as it is now.
    Deny {
        /// The document and why.
        #[command(flatten)]
        common: Common,
    },
    /// The document is to be replaced.
    Deprecate {
        /// The document and why.
        #[command(flatten)]
        common: Common,
        /// A document that replaces it (checked against its `replaces` links).
        #[arg(long)]
        successor: Option<Reference>,
    },
    /// The document is no longer kept.
    Retire {
        /// The document and why.
        #[command(flatten)]
        common: Common,
    },
    /// Withdraw the decision that stands about the document (needs --reason).
    Revoke {
        /// The document and why.
        #[command(flatten)]
        common: Common,
    },
    /// Declare that an edit keeps the meaning: the approvals that pinned the hash
    /// before it stay fresh, with a notice (needs --reason).
    Same {
        /// The documents edited, in order.
        #[arg(num_args = 0.., required_unless_present = "artifacts")]
        references: Vec<Reference>,
        /// An artifact, by its name as a link writes it (for the space `file`, its
        /// path under the root), in the one space the documents link it in; may be
        /// repeated.
        #[arg(long = "artifact", value_name = "NAME")]
        artifacts: Vec<ArtifactName>,
        /// The earlier hash the edit was read from (`sha256:...`), for one document or
        /// file, when approvals pinned several that do not mean the same.
        #[arg(long, value_name = "HASH")]
        from: Option<String>,
        /// Why the meaning is kept.
        #[arg(long, required = true)]
        reason: String,
        /// Who declares it (default: `WHYSHELF_ACTOR`, else Git's `user.name`).
        #[arg(long)]
        actor: Option<String>,
    },
    /// Withdraw decisions or declarations by the `id` of their record (needs
    /// --reason), with one revocation.
    Withdraw {
        /// The ids of the records, as answers and notices show them.
        #[arg(required = true, num_args = 1.., value_name = "RECORD_ID")]
        records: Vec<String>,
        /// Why.
        #[arg(long, required = true)]
        reason: String,
        /// Who withdraws them (default: `WHYSHELF_ACTOR`, else Git's `user.name`).
        #[arg(long)]
        actor: Option<String>,
    },
}
