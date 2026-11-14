//! The commands an answer gives as `next`, as they are typed: those that can settle
//! an origin of `stale` ([`commands`]), and those to run after `init` ([`after_init`]).
//!
//! What can settle an origin is decided once, by the model
//! ([`settlements`](whyshelf_core::domain::shelf::settle::settlements)); this module
//! only says each [`Settlement`] in the words of the command line, and decides
//! nothing about whether it applies.
//!
//! The words are drawn from the definition of the command line itself
//! ([`CommandFactory`]), by [`word`], so that a command or an argument renamed there
//! is renamed here, and a test parses every command written here with the same
//! definition. Nothing else in this module writes the name of a command or of an
//! argument.

use std::sync::LazyLock;

use clap::CommandFactory;
use whyshelf_core::app::shelf::Names;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::shelf::doc::DocKey;
use whyshelf_core::domain::shelf::settle::{SameSubject, Settlement};

use super::{NextRoot, shell_word};
use crate::args::Cli;

/// What a person writes in place of the reason that an edit keeps the meaning.
const WHY_SAME: &str = "<why the meaning is kept>";
/// What a person writes in place of the name an artifact moved to.
const NEW_NAME: &str = "<new name>";
/// What a person writes in place of the reason a moved artifact is the same thing.
const WHY_SAME_THING: &str = "<why it is the same thing>";
/// What a person writes in place of the reason one approval holds.
const WHY_HOLDS: &str = "<why it holds>";
/// What a person writes in place of the reason several approvals hold.
const WHY_THEY_HOLD: &str = "<why they hold>";
/// What a person writes in place of the time an approval lapses.
const UNTIL: &str = "<time>";

/// The words of a command or of one of its arguments, from the command line's own
/// definition: the names of the subcommands along `path` joined by a space (the
/// program's name for no path), or, with `argument`, the long name of the argument
/// with that id (the name of its field) of the command at `path`, as `--name`.
///
/// What the definition does not have is written as asked, which no command line
/// reads: the test that parses every command written here fails for it.
fn word(path: &[&str], argument: Option<&str>) -> String {
    found(path, argument).unwrap_or_else(|| match argument {
        Some(id) => format!("--{}", id.replace('_', "-")),
        None => path.join(" "),
    })
}

/// The definition of the command line, built once: every word of every command of
/// every origin is read from it.
static DEFINITION: LazyLock<clap::Command> = LazyLock::new(Cli::command);

/// [`word`], when the definition has what is asked.
fn found(path: &[&str], argument: Option<&str>) -> Option<String> {
    let mut command = &*DEFINITION;
    let mut names = Vec::new();
    for step in path {
        command = command.find_subcommand(step)?;
        names.push(command.get_name());
    }
    match argument {
        None if path.is_empty() => Some(command.get_name().to_owned()),
        None => Some(names.join(" ")),
        Some(id) => command
            .get_arguments()
            .find(|argument| argument.get_id().as_str() == id)
            .and_then(|argument| argument.get_long())
            .map(|long| format!("--{long}")),
    }
}

/// How a document is written in a command: its key in its book (`book:KEY`), which
/// no other document has and any reference reads; by its identity when it has no
/// valid key.
fn document(id: DocId, key: Option<&DocKey>) -> String {
    shell_word(&key.map_or_else(|| id.to_string(), ToString::to_string))
}

/// The program as the commands start, with the `--root` the run had; none when that
/// root is no text, since no command can be written with it. The one place a `next`
/// command's start is written, for `init` as for `stale`.
pub(super) fn invocation(root: NextRoot<'_>) -> Option<String> {
    let program = word(&[], None);
    match root {
        NextRoot::Current => Some(program),
        NextRoot::Given(dir) => Some(format!(
            "{program} {}={}",
            word(&[], Some("root")),
            shell_word(dir)
        )),
        NextRoot::NotText => None,
    }
}

/// One settlement as a command, after `whyshelf` (with its `--root`) as `program`.
fn command(settlement: &Settlement, names: &Names, program: &str) -> String {
    let doc = |id: &DocId| document(*id, names.key_of(*id));
    let same = ["judge", "same"];
    let approve = ["judge", "approve"];
    let reason = |path: &[&str]| word(path, Some("reason"));
    match settlement {
        Settlement::ReadDiff(id) => format!("{program} {} {}", word(&["diff"], None), doc(id)),
        Settlement::DeclareSame(SameSubject::Doc(id)) => format!(
            "{program} {} {} {} {}",
            word(&same, None),
            doc(id),
            reason(&same),
            shell_word(WHY_SAME)
        ),
        Settlement::DeclareSame(SameSubject::Artifact(artifact)) => format!(
            "{program} {} {} {} {} {}",
            word(&same, None),
            word(&same, Some("artifacts")),
            shell_word(artifact.name().as_str()),
            reason(&same),
            shell_word(WHY_SAME)
        ),
        Settlement::FindMoves => format!("{program} {}", word(&["moves"], None)),
        Settlement::DeclareMoved(artifact) => format!(
            "{program} {} {} {} {} {}",
            word(&["judge", "moved"], None),
            shell_word(artifact.name().as_str()),
            shell_word(NEW_NAME),
            reason(&["judge", "moved"]),
            shell_word(WHY_SAME_THING)
        ),
        Settlement::ApprovePending(id) => format!(
            "{program} {} {} {} {}",
            word(&approve, None),
            doc(id),
            reason(&approve),
            shell_word(WHY_HOLDS)
        ),
        Settlement::ApproveUntil(id) => format!(
            "{program} {} {} {} {} {} {}",
            word(&approve, None),
            doc(id),
            word(&approve, Some("until")),
            shell_word(UNTIL),
            reason(&approve),
            shell_word(WHY_HOLDS)
        ),
        Settlement::ApproveStale => format!(
            "{program} {} {} {} {}",
            word(&approve, None),
            word(&approve, Some("stale")),
            reason(&approve),
            shell_word(WHY_THEY_HOLD)
        ),
    }
}

/// The reason the `next` of `init` gives to the approval of the documents it made,
/// for a person to replace with theirs.
const INIT_REASON: &str = "read, and it holds";

/// The commands to run after `init`, as they are typed: `check` and `map`, then a
/// first document of `book` when `init` made none, or else the approval of `heads`,
/// the documents it made that no other of them stands on (so the run takes those and
/// the pending ones they stand on, upstream first, and nothing else). None when the
/// run's `--root` is no text.
pub(super) fn after_init(book: Option<&str>, heads: &[String], root: NextRoot<'_>) -> Vec<String> {
    let Some(program) = invocation(root) else {
        return Vec::new();
    };
    let approve = ["judge", "approve"];
    let mut next = vec![
        format!("{program} {}", word(&["check"], None)),
        format!("{program} {}", word(&["map"], None)),
    ];
    if heads.is_empty() {
        next.extend(book.map(|book| {
            format!(
                "{program} {} {book} {} \"A first {book}\"",
                word(&["new"], None),
                word(&["new"], Some("title"))
            )
        }));
    } else {
        next.push(format!(
            "{program} {} {} {} {} \"{INIT_REASON}\"",
            word(&approve, None),
            word(&approve, Some("pending")),
            heads.join(" "),
            word(&approve, Some("reason"))
        ));
    }
    next
}

/// The commands that can settle an origin, as they are typed, in the order the model
/// gives them. None when the run's `--root` is no text: no command can be written
/// with it.
pub(super) fn commands(
    settlements: &[Settlement],
    names: &Names,
    root: NextRoot<'_>,
) -> Vec<String> {
    let Some(program) = invocation(root) else {
        return Vec::new();
    };
    settlements
        .iter()
        .map(|settlement| command(settlement, names, &program))
        .collect()
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use whyshelf_core::domain::id::EntityId;
    use whyshelf_core::domain::shelf::artifact::{ArtifactKey, ArtifactName, Space};
    use whyshelf_core::domain::shelf::schema::{BookName, KeyPattern};

    use super::*;
    use crate::books::BooksCommand;
    use crate::books::args::Verb;

    fn docid(n: u8) -> DocId {
        DocId::new(
            format!("01900000-0000-7000-8000-0000000000{n:02x}")
                .parse::<EntityId>()
                .unwrap(),
        )
    }

    fn file(name: &str) -> ArtifactKey {
        ArtifactKey::new(
            Space::new("file").unwrap(),
            ArtifactName::new(name).unwrap(),
        )
    }

    fn key(book: &str, text: &str) -> DocKey {
        DocKey {
            book: BookName::new(book).unwrap(),
            form: KeyPattern::Slug.parse(text).unwrap(),
        }
    }

    /// Every settlement, one of each shape.
    fn every_settlement() -> Vec<Settlement> {
        vec![
            Settlement::ReadDiff(docid(1)),
            Settlement::DeclareSame(SameSubject::Doc(docid(1))),
            Settlement::DeclareSame(SameSubject::Artifact(file("tests/a.rs"))),
            Settlement::FindMoves,
            Settlement::DeclareMoved(file("tests/a.rs")),
            Settlement::ApprovePending(docid(1)),
            Settlement::ApproveUntil(docid(1)),
            Settlement::ApproveStale,
        ]
    }

    fn names() -> Names {
        Names::default()
    }

    /// The words a POSIX shell reads from a line of single-quoted and plain words.
    fn shell_words(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut word: Option<String> = None;
        let mut chars = line.chars();
        let mut quoted = false;
        // `init` writes its texts in double quotes, with nothing a shell reads in them.
        let mut double = false;
        while let Some(c) = chars.next() {
            match c {
                '\'' if !double => {
                    quoted = !quoted;
                    word.get_or_insert_with(String::new);
                }
                '"' if !quoted => {
                    double = !double;
                    word.get_or_insert_with(String::new);
                }
                '\\' if !quoted && !double => {
                    word.get_or_insert_with(String::new).extend(chars.next());
                }
                c if c.is_whitespace() && !quoted && !double => words.extend(word.take()),
                c => word.get_or_insert_with(String::new).push(c),
            }
        }
        assert!(!quoted && !double, "a quote is left open in {line}");
        words.extend(word);
        words
    }

    #[test]
    fn each_settlement_is_one_command_with_what_a_person_writes_in_angle_brackets() {
        let id = docid(1).to_string();
        let got = commands(&every_settlement(), &names(), NextRoot::Current);
        assert_eq!(
            got,
            [
                format!("whyshelf diff {id}"),
                format!("whyshelf judge same {id} --reason '<why the meaning is kept>'"),
                "whyshelf judge same --artifact tests/a.rs --reason '<why the meaning is kept>'"
                    .to_owned(),
                "whyshelf moves".to_owned(),
                "whyshelf judge moved tests/a.rs '<new name>' --reason '<why it is the same thing>'"
                    .to_owned(),
                format!("whyshelf judge approve {id} --reason '<why it holds>'"),
                format!("whyshelf judge approve {id} --until '<time>' --reason '<why it holds>'"),
                "whyshelf judge approve --stale --reason '<why they hold>'".to_owned(),
            ]
        );
    }

    #[test]
    fn the_root_the_run_had_is_carried_as_one_word_and_none_gives_no_command() {
        let settlements = [Settlement::FindMoves, Settlement::ApproveStale];
        assert_eq!(
            commands(&settlements, &names(), NextRoot::Given("books")),
            [
                "whyshelf --root=books moves",
                "whyshelf --root=books judge approve --stale --reason '<why they hold>'",
            ]
        );
        // A root with a space, and with a quote, is one word.
        let got = commands(&settlements[..1], &names(), NextRoot::Given("my 'dir"));
        assert_eq!(got, [r"whyshelf --root='my '\''dir' moves"]);
        assert_eq!(
            shell_words(&got[0]),
            ["whyshelf", "--root=my 'dir", "moves"]
        );
        // No text, no command (the run says so on stderr).
        assert_eq!(
            commands(&settlements, &names(), NextRoot::NotText),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_document_is_written_by_its_key_in_its_book_else_by_its_identity() {
        let id = docid(1);
        assert_eq!(document(id, None), id.to_string());
        assert_eq!(document(id, Some(&key("note", "one"))), "note:one");
        // The same key in two books is two references: the book tells them apart.
        assert_ne!(
            document(id, Some(&key("note", "one"))),
            document(docid(2), Some(&key("memo", "one")))
        );
    }

    #[test]
    fn the_names_are_drawn_from_the_definition_of_the_command_line() {
        assert_eq!(word(&[], None), "whyshelf");
        assert_eq!(word(&["judge", "approve"], None), "judge approve");
        assert_eq!(word(&["judge", "approve"], Some("stale")), "--stale");
        assert_eq!(word(&["judge", "approve"], Some("until")), "--until");
        // The id of the field of `--artifact`, which is not its long name.
        assert_eq!(word(&["judge", "same"], Some("artifacts")), "--artifact");
        assert_eq!(word(&[], Some("root")), "--root");
    }

    #[test]
    fn the_commands_after_init_parse_and_are_the_ones_meant() {
        let heads = vec!["F-1".to_owned(), "V-1".to_owned()];
        for root in [NextRoot::Current, NextRoot::Given("my dir")] {
            let first = after_init(Some("note"), &[], root);
            let made = after_init(Some("note"), &heads, root);
            for line in first.iter().chain(&made) {
                Cli::try_parse_from(shell_words(line))
                    .unwrap_or_else(|error| panic!("{line} does not parse: {error}"));
            }
            assert_eq!(first.len(), 3, "{first:?}");
            assert!(
                first[2].contains(" new note --title \"A first note\""),
                "{first:?}"
            );
            assert_eq!(made.len(), 3, "{made:?}");
            assert!(
                made[2]
                    .ends_with(" judge approve --pending F-1 V-1 --reason \"read, and it holds\""),
                "{made:?}"
            );
        }
        assert!(after_init(Some("note"), &heads, NextRoot::NotText).is_empty());
    }

    #[test]
    fn every_command_parses_with_the_placeholders_left_as_they_are() {
        for root in [
            NextRoot::Current,
            NextRoot::Given("books"),
            NextRoot::Given("my dir"),
        ] {
            for line in commands(&every_settlement(), &names(), root) {
                let words = shell_words(&line);
                let cli = Cli::try_parse_from(&words)
                    .unwrap_or_else(|error| panic!("{line} does not parse: {error}"));
                // What it parsed to is the command that was meant, not only some
                // command: the first word after the program and its `--root`.
                let first = words
                    .iter()
                    .skip(1)
                    .find(|w| !w.starts_with("--root="))
                    .map(String::as_str);
                let meant = matches!(
                    (&cli.command, first),
                    (BooksCommand::Diff { .. }, Some("diff"))
                        | (BooksCommand::Moves, Some("moves"))
                        | (
                            BooksCommand::Judge(
                                Verb::Same { .. } | Verb::Moved { .. } | Verb::Approve { .. }
                            ),
                            Some("judge")
                        )
                );
                assert!(meant, "{line} parsed as {:?}", cli.command);
                assert_eq!(cli.root.is_some(), matches!(root, NextRoot::Given(_)));
            }
        }
    }

    #[test]
    fn stale_gives_each_origin_its_commands_and_none_when_the_root_is_no_text() {
        use whyshelf_core::app::shelf::{Health, SettledOrigin, StaleReport};
        use whyshelf_core::domain::nonempty::NonEmptySet;
        use whyshelf_core::domain::shelf::freshness::{Origin, OriginEvent, OriginGroup};

        let report = StaleReport {
            entries: Vec::new(),
            origins: vec![
                SettledOrigin {
                    group: OriginGroup {
                        origin: Origin::Doc(docid(1)),
                        degraded: [docid(1)].into(),
                        held: NonEmptySet::new((OriginEvent::Edited, docid(1)), []),
                    },
                    next: vec![Settlement::FindMoves],
                },
                SettledOrigin {
                    group: OriginGroup {
                        origin: Origin::Doc(docid(2)),
                        degraded: [docid(2)].into(),
                        held: NonEmptySet::new((OriginEvent::NotValid, docid(2)), []),
                    },
                    next: Vec::new(),
                },
            ],
            unknown: Vec::new(),
            names: Names::default(),
            health: Health::default(),
        };
        let nexts = |root| {
            super::super::stale(&report, root)
                .origins
                .into_iter()
                .map(|origin| origin.next)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            nexts(NextRoot::Given("books")),
            [vec!["whyshelf --root=books moves".to_owned()], Vec::new()]
        );
        assert_eq!(nexts(NextRoot::NotText), [Vec::<String>::new(), Vec::new()]);
    }

    #[test]
    fn the_stale_approval_takes_no_document_and_the_until_one_takes_a_time() {
        let line = &commands(&[Settlement::ApproveStale], &names(), NextRoot::Current)[0];
        let cli = Cli::try_parse_from(shell_words(line)).unwrap();
        assert!(matches!(
            cli.command,
            BooksCommand::Judge(Verb::Approve {
                stale: true,
                ref references,
                until: None,
                ..
            }) if references.is_empty()
        ));
        let line = &commands(
            &[Settlement::ApproveUntil(docid(1))],
            &names(),
            NextRoot::Current,
        )[0];
        let cli = Cli::try_parse_from(shell_words(line)).unwrap();
        assert!(matches!(
            cli.command,
            BooksCommand::Judge(Verb::Approve {
                stale: false,
                ref references,
                until: Some(ref until),
                ..
            }) if references.len() == 1 && until == UNTIL
        ));
    }
}
