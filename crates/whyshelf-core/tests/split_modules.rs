//! The modules split along what they hold keep the shape the split gave them, read from
//! the source of the crates that hold them (this one and `whyshelf-cli`, whose module of
//! the answers is split the same way): the files of one module's children use each other
//! along the edges listed here and in no cycle, no child holds more restricted marks of
//! visibility (`pub(super)`, `pub(in ..)`) than listed or any `pub(crate)`, the parent's
//! public names are the ones listed, and every file a doc comment
//! names in backticks (`` `<path>.rs` ``) is there.
//!
//! The source is read by name, and no type is resolved: an edge is a name a child takes
//! from its parent (`use super::{..}`, `super::<name>`, or the parent's path written in
//! full) that a sibling defines, or a sibling's own name. A name two files share can make
//! a test fail where nothing is wrong; then the lists here are corrected, and never made
//! to pass in silence.

use std::collections::{BTreeMap, BTreeSet};

mod common;
use common::source::{code, entries, is_ident, is_tests, read, sources, workspace};

/// The source directories read, relative to the workspace's root.
const SOURCES: [&str; 2] = ["crates/whyshelf-core/src", "crates/whyshelf-cli/src"];

/// One module split into children, as the split left it.
struct Split {
    /// The source directory of the crate holding it, one of [`SOURCES`].
    src: &'static str,
    /// The parent's file under `src`, without `.rs`; its children are the files of the
    /// directory of that name, but those of tests (`tests.rs`, `*_tests.rs`).
    parent: &'static str,
    /// The parent's path written in full, as a child may name it instead of `super::`.
    path: &'static str,
    /// The edges between the children, `(user, used)`.
    edges: &'static [(&'static str, &'static str)],
    /// For each child, the most `pub(super)` and `pub(in ..)` it holds.
    widest: &'static [(&'static str, usize)],
    /// The parent's public names: its `pub` items and what it re-exports with `pub use`.
    public: &'static [&'static str],
}

const SPLITS: &[Split] = &[
    Split {
        src: SOURCES[0],
        parent: "domain/shelf/schema",
        path: "crate::domain::shelf::schema::",
        edges: &[
            ("validate", "decl"),
            ("validate", "field"),
            ("validate", "key"),
            ("validate", "link"),
            ("validate", "name"),
            ("validate", "word"),
            ("validate", "declared"),
            ("validate", "preset"),
            ("validate", "scope"),
            ("decl", "field"),
            ("decl", "key"),
            ("decl", "link"),
            ("decl", "scope"),
            ("preset", "decide"),
            ("preset", "decl"),
            ("preset", "link"),
            ("preset", "name"),
            ("preset", "word"),
            ("link", "key"),
            ("link", "name"),
            ("link", "word"),
            ("key", "name"),
            ("key", "word"),
            ("field", "name"),
            ("field", "word"),
            ("scope", "name"),
            ("scope", "word"),
        ],
        widest: &[
            ("decide", 1),
            ("declared", 3),
            ("key", 4),
            ("link", 8),
            ("name", 6),
            ("preset", 5),
            ("scope", 1),
            ("validate", 11),
            ("word", 3),
        ],
        public: &[
            "ARTIFACT_TARGET",
            "Approvals",
            "ArtifactCover",
            "ArtifactLinkKind",
            "ArtifactPropagation",
            "Authority",
            "BookDescription",
            "BookDir",
            "BookEntry",
            "BookName",
            "BookRange",
            "BookRuleDecl",
            "BookSchema",
            "BookSchemaDecl",
            "BookScope",
            "BookWord",
            "BuiltinPresets",
            "Cardinality",
            "CompareDecl",
            "CoverDecl",
            "CoverSeverity",
            "DecideDecl",
            "DecideKind",
            "DecideRule",
            "DeclWord",
            "Declared",
            "DeclaredLink",
            "DefaultsDecl",
            "DescriptionProblem",
            "EntryName",
            "Expanded",
            "FieldDecl",
            "FieldEntryDecl",
            "FieldPresence",
            "FieldType",
            "FieldTypeDecl",
            "FieldWord",
            "Group",
            "GroupDecl",
            "GroupName",
            "KeyForm",
            "KeyName",
            "KeyPattern",
            "KeyPatternDecl",
            "KeyPrefix",
            "KeyProblem",
            "KeyTail",
            "KeyWord",
            "LinkEntryDecl",
            "LinkKind",
            "LinkTargetDecl",
            "Member",
            "MemberDecl",
            "NamePlace",
            "NameProblem",
            "Pattern",
            "PatternProblem",
            "Permission",
            "Picked",
            "PresetDecl",
            "PresetName",
            "PresetOrigin",
            "PresetPlace",
            "PresetValues",
            "Propagation",
            "RuleDecl",
            "RulePlace",
            "RuleTime",
            "ScalarType",
            "Schema",
            "SchemaProblem",
            "ScopeDecl",
            "ScopeList",
            "ShelfDecl",
            "Slug",
            "WithoutFrontMatter",
            "WordAt",
            "WordOf",
            "Words",
            "WrittenBookDecl",
            "WrittenLinkDecl",
            "WrittenPresetValues",
            "expand",
            "read_book_word",
            "read_word",
        ],
    },
    Split {
        src: SOURCES[0],
        parent: "domain/shelf/ledger",
        path: "crate::domain::shelf::ledger::",
        edges: &[
            ("canon", "record"),
            ("canon", "decision"),
            ("canon", "pin"),
            ("canon", "judgement"),
            ("canon", "moved"),
            ("canon", "exempt"),
            ("canon", "same_meaning"),
            ("compared", "judgement"),
            ("compared", "moved"),
            ("compared", "pin"),
            ("compared", "refusal"),
            ("record", "decision"),
            ("record", "judgement"),
            ("record", "moved"),
            ("record", "exempt"),
            ("record", "same_meaning"),
            ("decision", "frame"),
            ("decision", "pin"),
            ("decision", "judgement"),
            ("frame", "pin"),
            ("moved", "decision"),
            ("moved", "judgement"),
            ("moved", "pin"),
            ("same_meaning", "compared"),
            ("same_meaning", "judgement"),
            ("same_meaning", "pin"),
            ("same_meaning", "refusal"),
            ("exempt", "judgement"),
            ("readers", "decision"),
            ("readers", "frame"),
            ("readers", "judgement"),
            ("readers", "moved"),
            ("readers", "pin"),
            ("admit", "decision"),
            ("admit", "judgement"),
            ("admit", "record"),
            ("admit", "same_meaning"),
        ],
        widest: &[
            ("admit", 5),
            ("compared", 42),
            ("decision", 3),
            ("exempt", 5),
            ("frame", 13),
            ("moved", 15),
            ("pin", 11),
            ("readers", 3),
            ("record", 6),
            ("refusal", 0),
            ("same_meaning", 13),
        ],
        public: &[
            "Admit",
            "AdmittedRecord",
            "Approval",
            "ApprovalKind",
            "ApprovalView",
            "ApproveFrame",
            "ArtifactPin",
            "Asked",
            "Before",
            "Decision",
            "DecisionProblem",
            "DecisionVerb",
            "DeclaredNow",
            "DeclaredStatus",
            "Exempt",
            "ExemptOf",
            "ExemptSubject",
            "FoundBy",
            "FramePin",
            "FrameRefusal",
            "Framed",
            "GoneName",
            "GoneNames",
            "JudgedAt",
            "JudgementId",
            "LeaveOut",
            "Ledger",
            "LedgerProblem",
            "MoveCandidate",
            "MoveNote",
            "MoveSuggestion",
            "MoveSuggestions",
            "Moved",
            "MovedOf",
            "MovedProblem",
            "MovedRefusal",
            "MovedTo",
            "NameSuggestions",
            "NamedRefusal",
            "NamedWhy",
            "NotAdmitted",
            "NotComparedWhy",
            "NotStanding",
            "NoteKind",
            "NothingWhy",
            "PATH_ONLY_SPACE",
            "PinnedArtifact",
            "PinName",
            "PinnedNames",
            "PinnedThere",
            "PrefixPlan",
            "PrefixRefusal",
            "PrefixSuggestion",
            "PresentFrame",
            "Quorum",
            "ReadBy",
            "ReadPin",
            "ReaderCount",
            "Readers",
            "Reading",
            "Record",
            "RecordBook",
            "RecordHash",
            "RecordOrder",
            "RecordProblem",
            "RefusedPrefix",
            "RetireFrame",
            "Revocation",
            "SameMeaning",
            "SameMeaningEdge",
            "SameMeaningOf",
            "SameMeaningProblem",
            "SameMeaningRefusal",
            "SameMeaningTarget",
            "Seen",
            "ShortSameMeaning",
            "Similarity",
            "SpaceSuggestions",
            "StandingApproval",
            "Taking",
            "WithdrawRefusal",
            "frame_approve",
            "frame_moved",
            "frame_prefix_moves",
            "frame_present",
            "frame_retire",
            "frame_same_meaning",
            "moved_space",
            "plan_prefix_moves",
        ],
    },
    // `gather` is a child, and no other module can make the `Gathered` it makes: its
    // fields are private to it, and what the parent reads of it is the three marks below.
    // `files` is the other: what the plan is as files, which reads the plan's own fields (a
    // child sees the private fields of its parent).
    Split {
        src: SOURCES[0],
        parent: "domain/shelf/rekey",
        path: "crate::domain::shelf::rekey::",
        edges: &[],
        widest: &[("gather", 3)],
        public: &[
            "Confirmed",
            "Contradicted",
            "Contradiction",
            "Difference",
            "Evidence",
            "Files",
            "FollowWhy",
            "Gathered",
            "Hints",
            "Item",
            "Meaning",
            "PlanDiffers",
            "Reading",
            "RekeyAsk",
            "RekeyPlan",
            "RekeyProblem",
            "Retargeted",
            "Rewriting",
            "Side",
            "StayWhy",
            "Undecided",
            "meaning",
            "plan",
        ],
    },
    // The parent re-exports only the names used through it from outside: the command line
    // is a binary, where a `pub use` nothing reaches is an unused import.
    Split {
        src: SOURCES[1],
        parent: "books/output",
        path: "crate::books::output::",
        edges: &[
            ("cause", "named"),
            ("cause", "words"),
            ("check", "cause"),
            ("check", "cover"),
            ("check", "doc"),
            ("check", "named"),
            ("check", "untold"),
            ("check", "words"),
            ("cover", "cause"),
            ("cover", "doc"),
            ("cover", "map"),
            ("cover", "words"),
            ("doc", "cause"),
            ("doc", "named"),
            ("doc", "words"),
            ("error", "rekey"),
            ("error", "words"),
            ("get", "doc"),
            ("init", "next"),
            ("judge", "cause"),
            ("judge", "named"),
            ("judge", "words"),
            ("map", "diagram"),
            ("map", "words"),
            ("moves", "doc"),
            ("moves", "named"),
            ("moves", "words"),
            ("new", "named"),
            ("next", "words"),
            ("reach", "doc"),
            ("reach", "named"),
            ("reach", "words"),
            ("rekey", "doc"),
            ("rekey", "named"),
            ("snapshot", "doc"),
            ("snapshot", "named"),
            ("snapshot", "words"),
            ("stale", "cause"),
            ("stale", "doc"),
            ("stale", "named"),
            ("stale", "next"),
            ("stale", "untold"),
            ("stats", "named"),
            ("stats", "summary"),
            ("status", "doc"),
            ("status", "words"),
            ("summary", "named"),
            ("trace", "doc"),
            ("trace", "named"),
            ("trace", "words"),
            ("unlinked", "doc"),
            ("unlinked", "words"),
            ("units", "doc"),
            ("units", "named"),
            ("units", "reach"),
            ("units", "words"),
            ("untold", "words"),
        ],
        widest: &[
            ("cause", 10),
            ("cover", 9),
            ("diagram", 1),
            ("doc", 8),
            ("map", 1),
            ("named", 2),
            ("next", 3),
            ("snapshot", 2),
            ("summary", 7),
            ("untold", 4),
            ("words", 8),
        ],
        public: &[
            "AbsentOut",
            "ApprovalsOut",
            "ApprovedOut",
            "ArtifactNamed",
            "BookApprovalsOut",
            "BookPendingOut",
            "BooksCheckOut",
            "CauseOut",
            "ChangedOut",
            "ChosenApprovedOut",
            "CoverOut",
            "CreatedOut",
            "DeclaredAllOut",
            "DiffOut",
            "DocApprovalsOut",
            "DocNamed",
            "DocRef",
            "ErrorOut",
            "ExemptOut",
            "ExemptedAllOut",
            "FORMAT_VERSION",
            "FmtOut",
            "GetOut",
            "HealthOut",
            "InitOut",
            "JudgedAllOut",
            "LinkOut",
            "LinkTargetOut",
            "MapOut",
            "MovedAllOut",
            "MovedNameOut",
            "MovedPrefixOut",
            "MovesOut",
            "NewManyOut",
            "NextRoot",
            "NoticeOut",
            "OriginCountOut",
            "ReachOut",
            "RekeyOut",
            "RepeatedOut",
            "RootOut",
            "RowOut",
            "RunOut",
            "RunsOut",
            "SchemasOut",
            "StaleOut",
            "StatsOut",
            "StatusOut",
            "TraceOut",
            "UnknownOut",
            "UnlinkedOut",
            "UnitsOut",
            "UnneededOut",
            "WithdrawnOut",
            "approved",
            "books_check",
            "check_notes",
            "check_warnings",
            "chosen_approved",
            "chosen_approved_warnings",
            "cover",
            "created",
            "diff",
            "error",
            "exempted",
            "failure",
            "fmt",
            "fmt_warnings",
            "get",
            "initialized",
            "judged",
            "judged_warnings",
            "lines_refused",
            "map",
            "moved",
            "moved_prefix",
            "moves",
            "new_many",
            "reach",
            "reach_warnings",
            "readers",
            "rekey",
            "same_meaning",
            "schemas",
            "stale",
            "stale_warnings",
            "standing",
            "stats",
            "status",
            "trace",
            "units",
            "units_warnings",
            "unlinked",
            "usage",
            "versioned",
            "withdrawn",
        ],
    },
];

/// The name a line of code at the top of a file defines, and whether it is plainly `pub`.
fn item(line: &str) -> Option<(&str, bool)> {
    let (rest, public) = if let Some(rest) = line.strip_prefix("pub(") {
        (rest.split_once(") ")?.1, false)
    } else if let Some(rest) = line.strip_prefix("pub ") {
        (rest, true)
    } else {
        (line, false)
    };
    let kinds = [
        "const fn ",
        "async fn ",
        "unsafe fn ",
        "fn ",
        "struct ",
        "enum ",
        "union ",
        "const ",
        "static ",
        "type ",
        "trait ",
        "macro_rules! ",
    ];
    let rest = kinds.iter().find_map(|kind| rest.strip_prefix(kind))?;
    let end = rest.find(|c: char| !is_ident(c)).unwrap_or(rest.len());
    (end > 0).then_some((&rest[..end], public))
}

fn defined(code: &str) -> BTreeSet<String> {
    code.lines()
        .filter_map(item)
        .map(|(name, _)| name.to_owned())
        .collect()
}

fn ident_at(text: &str) -> &str {
    let text = text.trim_start();
    &text[..text.find(|c: char| !is_ident(c)).unwrap_or(text.len())]
}

/// The items of a `{..}` list that begins `text`, split at its own commas, and the rest.
fn group(text: &str) -> (Vec<&str>, &str) {
    let mut items = Vec::new();
    let (mut depth, mut start) = (0, 1);
    for (at, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    items.push(&text[start..at]);
                    return (items, &text[at + 1..]);
                }
            }
            ',' if depth == 1 => {
                items.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    panic!("a `{{` with no `}}`: {text}");
}

/// The first names of the paths a child writes through its parent.
fn through_parent(code: &str, path: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for prefix in ["super::", path] {
        for (at, _) in code.match_indices(prefix) {
            let before = &code[..at];
            if before.ends_with("::") || before.ends_with(is_ident) {
                continue;
            }
            let rest = code[at + prefix.len()..].trim_start();
            let firsts = if rest.starts_with('{') {
                group(rest).0.into_iter().map(ident_at).collect()
            } else {
                vec![ident_at(rest)]
            };
            names.extend(
                firsts
                    .into_iter()
                    .filter(|name| !name.is_empty() && *name != "self" && *name != "super")
                    .map(str::to_owned),
            );
        }
    }
    names
}

/// `text` as names, `::` and single marks, without its white space.
fn tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let end = if is_ident(c) {
            rest.find(|c: char| !is_ident(c)).unwrap_or(rest.len())
        } else if rest.starts_with("::") {
            2
        } else {
            c.len_utf8()
        };
        if !c.is_whitespace() {
            tokens.push(&rest[..end]);
        }
        rest = &rest[end..];
    }
    tokens
}

/// The names a `pub use` of the parent re-exports (the last name of each path, or its
/// `as` name).
fn reexported(code: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for (at, _) in code.match_indices("\npub use ") {
        let text = &code[at + "\npub use ".len()..];
        let tokens = tokens(&text[..text.find(';').unwrap()]);
        for (k, token) in tokens.iter().enumerate() {
            let next = tokens.get(k + 1).copied();
            let name = token.starts_with(is_ident) && *token != "as" && *token != "self";
            if name && next != Some("::") && next != Some("as") {
                names.insert((*token).to_owned());
            }
        }
    }
    names
}

/// The children of `split`, each with its code.
fn children(split: &Split) -> BTreeMap<String, String> {
    entries(&workspace().join(split.src).join(split.parent))
        .into_iter()
        .filter(|path| path.extension().is_some_and(|e| e == "rs") && !is_tests(path))
        .map(|path| {
            let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
            (name, code(&read(&path)))
        })
        .collect()
}

fn parent_code(split: &Split) -> String {
    code(&read(
        &workspace()
            .join(split.src)
            .join(format!("{}.rs", split.parent)),
    ))
}

/// The edges between the children of `split`, each with a name that makes it.
fn edges(split: &Split) -> BTreeMap<(String, String), String> {
    let children = children(split);
    let parent = defined(&parent_code(split));
    let defs: BTreeMap<String, BTreeSet<String>> = children
        .iter()
        .map(|(name, code)| (name.clone(), defined(code)))
        .collect();
    let mut edges = BTreeMap::new();
    for (child, code) in &children {
        for name in through_parent(code, split.path) {
            if name == *child || defs[child].contains(&name) {
                continue;
            }
            if children.contains_key(&name) {
                edges.insert((child.clone(), name.clone()), name);
            } else if !parent.contains(&name) {
                for (other, names) in &defs {
                    if other != child && names.contains(&name) {
                        edges
                            .entry((child.clone(), other.clone()))
                            .or_insert_with(|| name.clone());
                    }
                }
            }
        }
    }
    edges
}

#[test]
fn the_children_of_a_split_module_use_each_other_in_no_cycle() {
    for split in SPLITS {
        let edges = edges(split);
        let mut left: BTreeSet<&String> = edges.keys().flat_map(|(a, b)| [a, b]).collect();
        loop {
            let leaves: Vec<&String> = left
                .iter()
                .copied()
                .filter(|node| {
                    !edges
                        .keys()
                        .any(|(user, used)| user == *node && left.contains(&used))
                })
                .collect();
            if leaves.is_empty() {
                break;
            }
            for leaf in leaves {
                left.remove(leaf);
            }
        }
        assert!(
            left.is_empty(),
            "{}: a cycle among {left:?} ({edges:?})",
            split.parent
        );
    }
}

#[test]
fn the_children_of_a_split_module_use_each_other_only_along_the_listed_edges() {
    let mut unlisted = Vec::new();
    for split in SPLITS {
        let listed: BTreeSet<(&str, &str)> = split.edges.iter().copied().collect();
        unlisted.extend(
            edges(split)
                .into_iter()
                .filter(|((user, used), _)| !listed.contains(&(user.as_str(), used.as_str())))
                .map(|((user, used), name)| format!("{}: {user} -> {used} ({name})", split.parent)),
        );
    }
    assert!(unlisted.is_empty(), "edges not listed: {unlisted:?}");
}

/// Counts the marks only: a mark changed to a bare `pub`, or one mark put for another,
/// is not caught here; the review reads those.
#[test]
fn no_child_of_a_split_module_holds_more_marks_of_visibility_than_listed() {
    let mut wider = Vec::new();
    for split in SPLITS {
        let widest: BTreeMap<&str, usize> = split.widest.iter().copied().collect();
        for (name, code) in children(split) {
            if code.contains("pub(crate)") {
                wider.push(format!("{}/{name}.rs: a `pub(crate)`", split.parent));
            }
            let count = code.matches("pub(super)").count() + code.matches("pub(in ").count();
            if count > widest.get(name.as_str()).copied().unwrap_or(0) {
                wider.push(format!("{}/{name}.rs: {count}", split.parent));
            }
        }
    }
    assert!(wider.is_empty(), "wider than listed: {wider:?}");
}

#[test]
fn a_split_module_keeps_its_public_names() {
    let mut changed = Vec::new();
    for split in SPLITS {
        let code = parent_code(split);
        let mut public = reexported(&code);
        public.extend(
            code.lines()
                .filter_map(item)
                .filter(|(_, public)| *public)
                .map(|(name, _)| name.to_owned()),
        );
        let listed: BTreeSet<String> = split.public.iter().map(|n| (*n).to_owned()).collect();
        if public != listed {
            changed.push(format!(
                "{}: gained {:?}, lost {:?}",
                split.parent,
                public.difference(&listed).collect::<Vec<_>>(),
                listed.difference(&public).collect::<Vec<_>>()
            ));
        }
    }
    assert!(changed.is_empty(), "public names changed: {changed:?}");
}

/// A file a doc comment names in backticks is there, under the directory of the file
/// holding the comment or one around it, up to the workspace's root. The files of tests
/// are left out: their docs name the files of their fixtures.
#[test]
#[arch_lint::allow(
    no_sync_io,
    reason = "a test with no async runtime looks for the files of the workspace's own source; whyshelf-core has no I/O crate"
)]
fn every_file_a_doc_comment_names_is_there() {
    let root = workspace();
    let mut missing = Vec::new();
    for file in SOURCES.iter().flat_map(|src| sources(&root.join(src))) {
        for (n, line) in read(&file).lines().enumerate() {
            let line = line.trim_start();
            if !(line.starts_with("//!") || line.starts_with("///")) {
                continue;
            }
            for named in line.split('`').skip(1).step_by(2) {
                let path_like = named.len() > ".rs".len()
                    && named.ends_with(".rs")
                    && named
                        .chars()
                        .all(|c| is_ident(c) || matches!(c, '/' | '.' | '-'));
                if !path_like {
                    continue;
                }
                let there = file
                    .parent()
                    .unwrap()
                    .ancestors()
                    .take_while(|dir| dir.starts_with(root))
                    .any(|dir| dir.join(named).is_file());
                if !there {
                    missing.push(format!("{}:{}: {named}", file.display(), n + 1));
                }
            }
        }
    }
    assert!(missing.is_empty(), "files named and not there: {missing:?}");
}
