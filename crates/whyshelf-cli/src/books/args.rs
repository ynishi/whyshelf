//! The commands, as clap types.

use std::str::FromStr;

use clap::builder::{PossibleValue, PossibleValuesParser, TypedValueParser};
use clap::error::ErrorKind;
use clap::{Args, Subcommand};
use whyshelf_core::domain::shelf::artifact::ArtifactName;
use whyshelf_core::domain::shelf::build::NEXT_KEY_TRIES;
use whyshelf_core::domain::shelf::lookup::Reference;
use whyshelf_core::domain::shelf::schema::{BookName, EntryName};
use whyshelf_infra::shelf::init::Template;

/// A link of a book as written in `--from`: `<book>.<link>`, read once by clap.
///
/// Neither grammar lets a name hold a `.` (a name is `a-z 0-9 - _`, at most 64
/// characters), so the text is split at its one `.`; a text with none, or more than
/// one, is no link of a book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookLink {
    /// The book that declares the link.
    pub book: BookName,
    /// The link.
    pub link: EntryName,
}

impl FromStr for BookLink {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (book, link) = text
            .split_once('.')
            .ok_or_else(|| format!("`{text}` is not `<book>.<link>`"))?;
        Ok(Self {
            book: BookName::new(book).map_err(|e| format!("the book of `{text}`: {e}"))?,
            link: EntryName::new(link).map_err(|e| format!("the link of `{text}`: {e}"))?,
        })
    }
}

/// The help of `new --key`, as `--help` shows it. The number of keys passed over is
/// the domain's ([`NEXT_KEY_TRIES`]), said here and not decided here, so the help
/// cannot differ from what `new` does.
fn key_help() -> String {
    format!(
        "The key (default: the book's next number; a book of slugs needs one). \
         Without --key, a key whose file is already there is passed over for the next \
         number, up to {NEXT_KEY_TRIES} keys."
    )
}

/// The name of a template of `init`, read by clap from the one closed set the
/// templates are ([`Template::ALL`]), so `--help` lists each with what it is, and a
/// name that is none is refused as every bad value is, with the names there are.
#[derive(Debug, Clone, Copy)]
pub struct TemplateName;

impl TemplateName {
    /// Each template as clap shows it: its name, and what it is.
    fn possible() -> impl Iterator<Item = PossibleValue> {
        Template::ALL
            .into_iter()
            .map(|t| PossibleValue::new(t.name()).help(t.about()))
    }
}

impl TypedValueParser for TemplateName {
    type Value = Template;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let name = PossibleValuesParser::new(Self::possible()).parse_ref(cmd, arg, value)?;
        // The parser above takes only the names of `Template::ALL`.
        Template::named(&name)
            .ok_or_else(|| clap::Error::new(ErrorKind::InvalidValue).with_cmd(cmd))
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(Self::possible()))
    }
}

/// The commands on the documents whose kinds the project declares in
/// `.whyshelf/books.toml`.
#[derive(Debug, Subcommand)]
pub enum BooksCommand {
    /// Start a project: write `.whyshelf/books.toml` and the documents of a template
    /// beside it, with the files they name (by default `minimal`, one book of notes with comments that show how
    /// to extend it), where no declaration is; it never writes over a file, and
    /// writes no approval.
    Init {
        /// The template to write.
        #[arg(long, value_name = "NAME", value_parser = TemplateName)]
        template: Option<Template>,
        /// A shorthand for `--template sample`.
        #[arg(long, conflicts_with = "template")]
        sample: bool,
    },
    /// Check the declaration, the documents and the ledger; exit 1 if anything is
    /// wrong.
    Check,
    /// What the declaration says: every book and every link, with a drawing of
    /// them; it reads nothing else, and exits 1 as `check` does when the
    /// declaration cannot be used.
    Map,
    /// What the covering links to artifacts expect, and how each stands: for each
    /// link that declares a `cover`, the selection of names it takes, whether every
    /// name it takes is listed, and which names are reached by a document, exempt or
    /// a gap. A gap, a listing that is not the whole and a selection that takes
    /// nothing are told, not failed (`check` fails them); exit 1 only when the
    /// declaration cannot be used, as for `map`.
    Cover {
        /// Only this link, as `<book>.<link>`: the book that declares it, and its
        /// name. One that is no link to artifacts with a `cover` is a request error
        /// (exit 2).
        #[arg(long, value_name = "BOOK.LINK", value_parser = BookLink::from_str)]
        link: Option<BookLink>,
    },
    /// The documents, what was decided about each and whether it still holds; every
    /// document unless filtered. A document must pass every kind of filter given, and
    /// any one of the values of a kind repeated. `health` is of the whole project,
    /// not of the documents listed. A book or a field that is not declared, a value
    /// no book reads as the field's type, a value left empty (`--where field=`), a
    /// field asked with a value (`--where`, `--with`) and with no value, no value
    /// asked of a field every
    /// book that declares it requires, or a word that is no standing is a request
    /// error (exit 2).
    Status {
        /// Only the documents of this book; repeat it for any of several books.
        #[arg(long = "book", value_name = "NAME")]
        books: Vec<String>,
        /// Only the documents at this standing, in the word an answer gives as a
        /// document's `standing` (a word that is none is told with the words there
        /// are); repeat it for any of several.
        #[arg(long = "standing", value_name = "WORD")]
        standings: Vec<String>,
        /// Only the documents of a book that declares the field, with the value
        /// given: equal to it, or for a list field holding it as an item; the text
        /// after the first `=` is the value. Repeat it for each condition a
        /// document must hold.
        #[arg(long = "where", value_name = "FIELD=VALUE")]
        wheres: Vec<String>,
        /// Only the documents of a book that declares the field, which hold no
        /// value for it: they do not write it, or write it with nothing in it. Repeat
        /// it for each field a document must hold none of; it is one more condition
        /// beside the others, as `--where` is. A field no book declares, one that
        /// is also asked with `--where`, and one that every book that declares it
        /// requires are request errors (exit 2).
        #[arg(long = "without", value_name = "FIELD")]
        withouts: Vec<String>,
        /// Only the documents of a book that declares the field, which hold a value
        /// for it, whichever value: they write it with something in it. Repeat it for
        /// each field a document must hold; it is one more condition beside the
        /// others, as `--where` is, and may be given with `--where` on the same field.
        /// A field no book declares, and one that is also asked with `--without`, are
        /// request errors (exit 2). It is for finding the documents that hold a field
        /// before the declaration of it is taken out.
        #[arg(long = "with", value_name = "FIELD")]
        withs: Vec<String>,
    },
    /// One document in full, with where its approval stops holding.
    Get {
        /// A key (`REQ-1`, `requirement:REQ-1`), an identifier or a short identifier.
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
        /// Exit 1 when any approval does not hold (as `git diff --exit-code`), or
        /// stands on a name that cannot be told now, so a script or CI can fail on
        /// it; without it the exit is 0.
        #[arg(long)]
        exit_code: bool,
    },
    /// How the work went, from the ledger and the documents: approvals made again,
    /// the time documents stayed pending, the runs of `judge` (one that writes no
    /// decision, as `revoke`, `same` and `withdraw`, is no run here), and the
    /// documents and artifacts that changed under approvals made again; it writes
    /// nothing.
    Stats {
        /// Only the records at or after this time: an RFC 3339 time with an offset or
        /// `Z` (`2026-11-11T00:00:00+09:00`, `2026-11-11T00:00:00Z`), or a date
        /// `YYYY-MM-DD`, the start of that day in UTC. A time with no offset is
        /// refused.
        #[arg(long)]
        since: Option<String>,
    },
    /// The names approvals pinned something at that have nothing behind them now
    /// (read through the moves declared), each with the documents that pin it and
    /// where its space's adapter suggests it moved (for files: the same content, then
    /// Git's renames since the approval); it declares nothing and writes nothing.
    Moves,
    /// Each item of the approval that stands about a document, as it was approved and
    /// as it is now, with a line diff where both are text.
    Diff {
        /// A key, an identifier or a short identifier.
        reference: Reference,
    },
    /// Record a judgement about a document (it, `new`, `init` and `fmt` are the only
    /// ones of these that write).
    #[command(subcommand)]
    Judge(Verb),
    /// Create one new document, valid for its book, at `<book dir>/<key>.md` (never
    /// in place of a file that is there), or with `--from` every document of a file
    /// of JSON lines, all or none.
    New {
        /// The book.
        #[arg(required_unless_present = "from")]
        book: Option<String>,
        /// A shorthand for the entry `title=<text>`, for a book that declares a
        /// `title` field; a book declares whether a title is required.
        #[arg(long)]
        title: Option<String>,
        /// The key; its help, as `--help` shows it, is `key_help`.
        #[arg(long, help = key_help())]
        key: Option<String>,
        /// More entries, `<entry>=<value>`. A list field or a link takes `a,b`, or one
        /// item at a time with `<entry>+=<item>`, commas kept; a truth value `true` or
        /// `false`; a whole number its digits. Links are keys.
        entries: Vec<String>,
        /// The body.
        #[arg(long)]
        body: Option<String>,
        /// Write the documents of this file, one JSON object per line (`-` is the
        /// standard input): each line has `book`, `key`, `as`, `entries` and `body`,
        /// and an item of a link may be `{"as": "<name>"}`, the document of the line
        /// of that name. The shape of a line is `inputs."new --from"` of `schema`.
        /// Every line is judged before any file is written, and any line refused
        /// writes nothing; the shelf is read once. Not given with a book, `--title`,
        /// `--key`, entries or `--body`.
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with_all = ["book", "title", "key", "entries", "body"]
        )]
        from: Option<String>,
    },
    /// Write every document's file in the one form `new` writes (`id` and `key`
    /// first, every text in double quotes, the whole text normalized as it is read:
    /// `\n` line endings, no trailing white space), when that changes no
    /// reading of it; a file it cannot write so is left as it is, and listed with why.
    /// With `--drop NAME` it also takes out of each file the entry of that name, when
    /// the book of the file declares no field, link or link to artifacts of that name,
    /// and says which names it took out of which file. A name that is `id` or `key`,
    /// or that every book declares, or that is empty is a request error (exit 2),
    /// and nothing is written.
    Fmt {
        /// Write nothing: list the files not in the form, those it would write and
        /// those it would leave alone apart, and exit 1 when there is any of either;
        /// with `--drop`, the names it would take out of each file too.
        #[arg(long)]
        check: bool,
        /// Take the entry of this name out of every file whose book does not declare
        /// it (a field, a link or a link to artifacts taken out of the declaration of
        /// the books); an entry of that name in a book that declares it stays. Repeat
        /// it for each name.
        #[arg(long = "drop", value_name = "NAME")]
        drops: Vec<String>,
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
    /// Who ran the command, when not the one who answers for it (default:
    /// `WHYSHELF_RAN_BY`, else no one).
    #[arg(long = "ran-by", value_name = "NAME")]
    pub ran_by: Option<String>,
}

/// The verbs of a judgement.
#[derive(Debug, Subcommand)]
pub enum Verb {
    /// The document is sound as it is now.
    #[command(override_usage = "\
whyshelf judge approve <REFERENCES>... [--again] [--reason <REASON>] [--actor <ACTOR>] [--ran-by <NAME>] [--until <UNTIL>]
       whyshelf judge approve --stale --reason <REASON> [--actor <ACTOR>] [--ran-by <NAME>] [--until <UNTIL>]
       whyshelf judge approve --pending [<REFERENCES>...] [--stale] --reason <REASON> [--actor <ACTOR>] [--ran-by <NAME>] [--until <UNTIL>]")]
    Approve {
        /// The documents, as for every verb ([`Common::references`]); with
        /// --pending, the pending documents to approve with the pending ones they
        /// stand on; none with --stale alone.
        #[arg(num_args = 1.., required_unless_present_any = ["stale", "pending"])]
        references: Vec<Reference>,
        /// Approve again, in one run and upstream first by the links, every approval
        /// that is expired or stale and would hold once approved; the rest are left
        /// out, each with a warning on stderr (needs --reason, takes no document).
        #[arg(long, requires = "reason")]
        stale: bool,
        /// Approve, in one run and upstream first by the links, every pending
        /// document (never approved, or every approval withdrawn) that would hold
        /// once approved, or only the documents named, each pending, and the pending
        /// documents they stand on through `depends` links; the rest are left out,
        /// each with a warning on stderr (needs --reason; may be given with --stale,
        /// as one run).
        #[arg(long, requires = "reason")]
        pending: bool,
        /// Approve even a document whose approval would only repeat the actor's own
        /// (same pins, same expiry, by one who approved that content), for a person
        /// who read it again; without it, such a document is left out, with a
        /// warning on stderr. Another actor's approval of the same content is
        /// written without it.
        #[arg(long, conflicts_with_all = ["stale", "pending"])]
        again: bool,
        /// Why.
        #[arg(long)]
        reason: Option<String>,
        /// Who judges (default: `WHYSHELF_ACTOR`, else Git's `user.name`).
        #[arg(long)]
        actor: Option<String>,
        /// Who ran the command, when not the one who answers for it (default:
        /// `WHYSHELF_RAN_BY`, else no one).
        #[arg(long = "ran-by", value_name = "NAME")]
        ran_by: Option<String>,
        /// When the approval lapses: an RFC 3339 time with an offset or `Z`
        /// (`2026-11-11T00:00:00+09:00`, `2026-11-11T00:00:00Z`), or a date
        /// `YYYY-MM-DD`, the start of that day in UTC. A time with no offset is
        /// refused.
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
        /// Who ran the command, when not the one who answers for it (default:
        /// `WHYSHELF_RAN_BY`, else no one).
        #[arg(long = "ran-by", value_name = "NAME")]
        ran_by: Option<String>,
    },
    /// Declare that an artifact moved from one name to another (needs --reason), one
    /// move per run: an approval's pin of something at the old name, made at or
    /// before the declaration, is read as a pin of the new one, with the hash it
    /// pinned (a pin of the old name as absent is read as written). The old
    /// name has nothing behind it now and the new one has something, as the space's
    /// adapter says. With `--prefix`, the two are prefixes, and every name an approval
    /// pins under the first (in any space) is declared moved to the same name under
    /// the second, one declaration for each, in one run: all or none.
    Moved {
        /// Move every name under a prefix: OLD and NEW are prefixes (a directory, a
        /// module, a file and a path in it), and each space's adapter says which of the
        /// names pinned are under OLD and what each is under NEW.
        #[arg(long)]
        prefix: bool,
        /// The old name (for the space `file`, its path under the root): the name a
        /// pin of something there, of an approval that stands, is read as now, in the
        /// one space it is in. With `--prefix`, the prefix the names are under now.
        #[arg(value_name = "OLD")]
        from: String,
        /// The new name, in the same space. With `--prefix`, the prefix they are
        /// moved under.
        #[arg(value_name = "NEW")]
        to: String,
        /// Why it is the same thing.
        #[arg(long, required = true)]
        reason: String,
        /// Who declares it (default: `WHYSHELF_ACTOR`, else Git's `user.name`).
        #[arg(long)]
        actor: Option<String>,
        /// Who ran the command, when not the one who answers for it (default:
        /// `WHYSHELF_RAN_BY`, else no one).
        #[arg(long = "ran-by", value_name = "NAME")]
        ran_by: Option<String>,
    },
    /// Declare that each document, or each artifact with `--artifact`, is not expected
    /// to be reached by one covering link (needs --reason): it is not listed in
    /// `check`'s `uncovered` (`artifact_uncovered`) for that link, and does not fail
    /// `check` under `cover = true`. The link is declared with a cover and points
    /// into the document's book (or to artifacts, for an artifact), does not reach
    /// the subject now, and a document is not decided against; an artifact is one
    /// the link's selection takes now.
    Exempt {
        /// The documents, in order: keys, identifiers or short identifiers. Not given
        /// with `--artifact`.
        #[arg(num_args = 0.., required_unless_present = "artifacts", conflicts_with = "artifacts")]
        references: Vec<Reference>,
        /// An artifact, by its name as a link writes it (for the space `file`, its
        /// path under the root), in the space of the link; may be repeated. Not given
        /// with document references.
        #[arg(long = "artifact", value_name = "NAME")]
        artifacts: Vec<ArtifactName>,
        /// The covering link, as `<book>.<link>`: the book that declares it, and its
        /// name.
        #[arg(long, required = true, value_name = "BOOK.LINK", value_parser = BookLink::from_str)]
        from: BookLink,
        /// Why the document is meant to stay unreached.
        #[arg(long, required = true)]
        reason: String,
        /// Who declares it (default: `WHYSHELF_ACTOR`, else Git's `user.name`).
        #[arg(long)]
        actor: Option<String>,
        /// Who ran the command, when not the one who answers for it (default:
        /// `WHYSHELF_RAN_BY`, else no one).
        #[arg(long = "ran-by", value_name = "NAME")]
        ran_by: Option<String>,
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
        /// Who ran the command, when not the one who answers for it (default:
        /// `WHYSHELF_RAN_BY`, else no one).
        #[arg(long = "ran-by", value_name = "NAME")]
        ran_by: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_book_link_is_split_at_its_one_dot_with_names_of_the_most_characters() {
        // A name is at most 64 characters of `a-z 0-9 - _`; neither holds a `.`.
        let book = "b".repeat(64);
        let link = "l_-9".repeat(16);
        let parsed: BookLink = format!("{book}.{link}").parse().unwrap();
        assert_eq!(parsed.book.as_str(), book);
        assert_eq!(parsed.link.as_str(), link);

        for refused in [
            format!("{}.{link}", "b".repeat(65)),
            format!("{book}.{}", "l".repeat(65)),
            "note".to_owned(),
            "note.see.more".to_owned(),
            ".see".to_owned(),
            "note.".to_owned(),
            "note.key".to_owned(),
            "No.see".to_owned(),
        ] {
            assert!(refused.parse::<BookLink>().is_err(), "{refused}");
        }
    }
}
