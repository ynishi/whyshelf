//! The arguments of `whyshelf books ...`, as clap types.

use clap::{Args, Subcommand};

/// The commands on a shelf of books: documents whose kinds the project declares in
/// `.whyshelf/books.toml`.
#[derive(Debug, Subcommand)]
pub enum BooksCommand {
    /// Check the declaration, the documents and the ledger; exit 1 if anything is
    /// wrong.
    Check,
    /// Every document, what was decided about it and whether it still holds.
    Status,
    /// One document in full, with where its approval stops holding.
    Get {
        /// A key (`S-1`, `spec:S-1`), an identifier or a short identifier.
        reference: String,
    },
    /// What a document links to and what links to it, transitively.
    Trace {
        /// A key, an identifier or a short identifier.
        reference: String,
    },
    /// The approvals that no longer hold, with where each starts to fail.
    Stale,
    /// Record a judgement about a document (the only one of these that writes).
    #[command(subcommand)]
    Judge(Verb),
    /// The JSON Schema of the answer of every `books` command.
    Schema,
}

/// What every verb takes.
#[derive(Debug, Args)]
pub struct Common {
    /// The document: a key, an identifier or a short identifier.
    pub reference: String,
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
        successor: Option<String>,
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
}
