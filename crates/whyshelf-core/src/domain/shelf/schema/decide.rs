//! Who may decide about the documents of a book: the groups of names a project
//! declares ([`Group`]), and, for a book and for the records of no book, which groups
//! may write each kind of judgement ([`DecideRule`]), read together as the project's
//! [`Authority`]. Whether one record is admitted is the ledger's question, asked of
//! [`Authority::permission`], the one place a rule is read.
//!
//! ```toml
//! [[group]]
//! name = "maintainers"
//! members = ["ana", "bo"]
//!
//! [[group]]
//! name = "agents"
//! members = ["coder", { name = "old-coder", until = "2026-12-01" }]
//!
//! [decide]                        # the records of no book, and of a book that cannot be told
//! default = ["maintainers"]
//!
//! [[book]]
//! name = "requirement"
//! # ...
//! [book.decide]
//! approve = ["maintainers"]       # who may approve the documents of the book (and declare
//!                                 # the same meaning of one, as `same` is not named)
//! default = ["maintainers", "agents"]   # the kinds the rule does not name
//! since = "2026-12-01"            # the records made from then on; RFC 3339 also
//! ```
//!
//! # No authentication
//!
//! The rule is about the name a record writes, as it is declared: the `actor` of a
//! record is taken as written, like a Git author ([`Actor`]), and so is its time. One
//! person who writes another name, or another time, is read as that name at that time.
//! A rule is an arrangement among contributors who mean to keep it, checked against the
//! ledger; it does not stop someone who means to break it, and it is no proof that the
//! person named made the record.
//!
//! # The rules
//!
//! - A group has a name, used once, and one or more members, each an exact name of an
//!   actor (no pattern: a name is a member, or is not). A member may have a `from` and
//!   an `until`: it is in the group for the records made at or after `from` and before
//!   `until`, and `from` is before `until`. A member that leaves is kept with an
//!   `until`, not removed: removing it reads every record it made again under the
//!   rule.
//! - A rule names, for each kind of judgement ([`DecideKind`]), the groups whose
//!   members may write it, and a `default` for the kinds it does not name. Every group
//!   it names is declared. `same` not named takes the groups of `approve`, then the
//!   default, since a declaration of the same meaning keeps an approval standing.
//! - A book with no rule is unruled: any name may write any record about its
//!   documents, as before rules existed. A kind a rule neither names nor covers with a
//!   default is open: any name may write it.
//! - A rule with `since` reads only the records made at or after it; those made before
//!   are unruled. Moving `since` later admits again what an earlier rule refused.
//! - When any book declares a rule, `[decide]` is declared too: it rules the records
//!   of no book (a declaration of a move, of the same meaning of an artifact) and those
//!   whose book cannot be told, so that no record escapes every rule.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::actor::Actor;
use crate::domain::nonempty::{NonEmptySet, NonEmptyVec};

use super::{BookName, GroupName, NamePlace, SchemaProblem};

/// A moment a rule names (`since`, a member's `from` and `until`), in unix
/// milliseconds, as an adapter read it from what the declaration writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleTime(i64);

impl RuleTime {
    /// The moment `millis` milliseconds after the unix epoch.
    pub fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    /// The milliseconds after the unix epoch.
    pub fn millis(self) -> i64 {
        self.0
    }
}

impl fmt::Display for RuleTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unix ms {}", self.0)
    }
}

/// The kind of judgement a record is, as a rule names it: one per kind of record and
/// verb of a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DecideKind {
    /// An approval.
    Approve,
    /// A denial.
    Deny,
    /// A deprecation.
    Deprecate,
    /// A retirement.
    Retire,
    /// A revocation, of one record it names.
    Revoke,
    /// A declaration that an edit keeps the meaning.
    Same,
    /// An exemption from a covering link.
    Exempt,
    /// A declaration that an artifact moved.
    Moved,
}

impl DecideKind {
    /// Every kind, in the order a declaration lists them.
    pub const ALL: [Self; 8] = [
        Self::Approve,
        Self::Deny,
        Self::Deprecate,
        Self::Retire,
        Self::Revoke,
        Self::Same,
        Self::Exempt,
        Self::Moved,
    ];

    /// The word a declaration writes for it, the key of the rule.
    pub fn word(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
            Self::Deprecate => "deprecate",
            Self::Retire => "retire",
            Self::Revoke => "revoke",
            Self::Same => "same",
            Self::Exempt => "exempt",
            Self::Moved => "moved",
        }
    }

    /// What a record of the kind does, said after "may not": `approve`, `declare a
    /// same meaning`, `declare a move`, ...
    pub fn deed(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
            Self::Deprecate => "deprecate",
            Self::Retire => "retire",
            Self::Revoke => "revoke",
            Self::Same => "declare a same meaning",
            Self::Exempt => "exempt",
            Self::Moved => "declare a move",
        }
    }
}

impl fmt::Display for DecideKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// A member of a group: an exact name, in the group from `from` (when given) until
/// `until` (when given).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    name: Actor,
    from: Option<RuleTime>,
    until: Option<RuleTime>,
}

impl Member {
    /// The name.
    pub fn name(&self) -> &Actor {
        &self.name
    }

    /// From when it is in the group, if not always.
    pub fn from(&self) -> Option<RuleTime> {
        self.from
    }

    /// Until when it is in the group, if not always.
    pub fn until(&self) -> Option<RuleTime> {
        self.until
    }

    /// Whether `actor` is this member for a record made at `at`.
    fn holds(&self, actor: &Actor, at: RuleTime) -> bool {
        self.name == *actor
            && self.from.is_none_or(|from| from <= at)
            && self.until.is_none_or(|until| at < until)
    }
}

/// A group of names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    name: GroupName,
    members: NonEmptyVec<Member>,
}

impl Group {
    /// The name.
    pub fn name(&self) -> &GroupName {
        &self.name
    }

    /// The members, in the order written.
    pub fn members(&self) -> &NonEmptyVec<Member> {
        &self.members
    }
}

/// Which groups may write each kind of judgement, for one book or for the records of
/// no book.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecideRule {
    kinds: BTreeMap<DecideKind, NonEmptySet<GroupName>>,
    default: Option<NonEmptySet<GroupName>>,
    since: Option<RuleTime>,
}

impl DecideRule {
    /// The groups named for one kind.
    pub fn named(&self, kind: DecideKind) -> Option<&NonEmptySet<GroupName>> {
        self.kinds.get(&kind)
    }

    /// The groups of the kinds not named.
    pub fn default_groups(&self) -> Option<&NonEmptySet<GroupName>> {
        self.default.as_ref()
    }

    /// From when the rule reads records, if not always.
    pub fn since(&self) -> Option<RuleTime> {
        self.since
    }

    /// The groups whose members may write `kind`: those named for it; for `same` not
    /// named, those of `approve`; else the default. None is open.
    fn groups_of(&self, kind: DecideKind) -> Option<&NonEmptySet<GroupName>> {
        self.kinds
            .get(&kind)
            .or_else(|| match kind {
                DecideKind::Same => self.kinds.get(&DecideKind::Approve),
                _ => None,
            })
            .or(self.default.as_ref())
    }
}

/// What the rules say of a record ([`Authority::permission`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    /// No rule reads it: the book has none, or the record was made before its
    /// `since`.
    Unruled,
    /// A rule reads it and names no group for its kind, nor a default: any name may.
    Open,
    /// A member of a group the rule names for its kind wrote it.
    Admitted,
    /// No member of the groups the rule names for its kind wrote it.
    Refused {
        /// The groups whose members may.
        groups: NonEmptySet<GroupName>,
        /// From when the rule reads records, if not always.
        since: Option<RuleTime>,
    },
}

/// Who may decide, for the whole project: the groups, the rule of each book that
/// declares one, and the rule of the records of no book (`[decide]`). A project that
/// declares none is unruled throughout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Authority {
    groups: BTreeMap<GroupName, Group>,
    books: BTreeMap<BookName, DecideRule>,
    shelf: Option<DecideRule>,
}

impl Authority {
    /// Whether no rule is declared at all: every record is unruled.
    pub fn is_unruled(&self) -> bool {
        self.books.is_empty() && self.shelf.is_none()
    }

    /// The groups, by name.
    pub fn groups(&self) -> &BTreeMap<GroupName, Group> {
        &self.groups
    }

    /// The rule of `book`, or of the records of no book when `book` is none.
    pub fn rule_of(&self, book: Option<&BookName>) -> Option<&DecideRule> {
        match book {
            Some(book) => self.books.get(book),
            None => self.shelf.as_ref(),
        }
    }

    /// What the rule of `book` (of the records of no book, when none) says of a
    /// record of `kind` written by `actor` at `at`: the one place a rule is read (the
    /// rules are on the module page).
    pub fn permission(
        &self,
        book: Option<&BookName>,
        kind: DecideKind,
        actor: &Actor,
        at: RuleTime,
    ) -> Permission {
        let Some(rule) = self.rule_of(book) else {
            return Permission::Unruled;
        };
        if rule.since.is_some_and(|since| at < since) {
            return Permission::Unruled;
        }
        let Some(groups) = rule.groups_of(kind) else {
            return Permission::Open;
        };
        let member = groups.iter().any(|name| {
            self.groups
                .get(name)
                .is_some_and(|group| group.members.iter().any(|m| m.holds(actor, at)))
        });
        if member {
            Permission::Admitted
        } else {
            Permission::Refused {
                groups: groups.clone(),
                since: rule.since,
            }
        }
    }
}

/// Where a rule of who may decide is declared, for a problem to say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulePlace {
    /// `[decide]`, the rule of the records of no book.
    Shelf,
    /// `[book.decide]` of the book, as written.
    Book(String),
}

impl fmt::Display for RulePlace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shelf => f.write_str("`[decide]`"),
            Self::Book(book) => write!(f, "book `{book}`: `[book.decide]`"),
        }
    }
}

/// Who may decide, as an adapter read it: nothing checked but the shape (the kinds
/// are the keys a rule may write, and the times are read).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecideDecl {
    /// The groups, in the order written.
    pub groups: Vec<GroupDecl>,
    /// `[decide]`, when written.
    pub shelf: Option<RuleDecl>,
    /// `[book.decide]` of each book that writes one.
    pub books: Vec<BookRuleDecl>,
}

/// A group as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupDecl {
    /// The name.
    pub name: String,
    /// The members.
    pub members: Vec<MemberDecl>,
}

/// A member as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberDecl {
    /// The name.
    pub name: String,
    /// `from`, when written.
    pub from: Option<RuleTime>,
    /// `until`, when written.
    pub until: Option<RuleTime>,
}

/// A rule as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleDecl {
    /// The groups named for each kind it writes.
    pub kinds: Vec<(DecideKind, Vec<String>)>,
    /// The groups of the kinds it does not name, when written.
    pub default: Option<Vec<String>>,
    /// `since`, when written.
    pub since: Option<RuleTime>,
}

/// The rule of one book, with the book's name as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookRuleDecl {
    /// The book.
    pub book: String,
    /// Its rule.
    pub rule: RuleDecl,
}

/// The authority `decl` declares, every problem of it pushed to `problems`: a group
/// whose name breaks the grammar or is used twice, that has no member, a member that
/// is no name of an actor or whose `from` is not before its `until`, a rule that
/// names a group not declared or a list of no group, a rule of a book `books` does
/// not hold (the names of the books as written) or of one book twice, and rules of
/// books with no `[decide]`. What is wrong is left out of what is returned, which is
/// used only when nothing is.
pub(super) fn read_authority(
    decl: DecideDecl,
    books: &BTreeSet<String>,
    problems: &mut Vec<SchemaProblem>,
) -> Authority {
    let mut groups = BTreeMap::new();
    // Every group as written, read or not: one with problems of its own is declared,
    // so a second of its name is a duplicate and a rule naming it names no unknown.
    let mut written = BTreeSet::new();
    for group in decl.groups {
        let first = written.insert(group.name.clone());
        if let Some(group) = read_group(group, problems) {
            if first {
                groups.insert(group.name.clone(), group);
            } else {
                problems.push(SchemaProblem::DuplicateGroup {
                    name: group.name.clone(),
                });
            }
        }
    }
    let declared = Groups {
        read: &groups,
        written: &written,
    };
    let mut rules = BTreeMap::new();
    let ruled_books = !decl.books.is_empty();
    for BookRuleDecl { book, rule } in decl.books {
        let place = RulePlace::Book(book.clone());
        let name = match BookName::new(&book) {
            Ok(name) if books.contains(&book) => name,
            _ => {
                problems.push(SchemaProblem::DecideOfUnknownBook { book });
                continue;
            }
        };
        if rules.contains_key(&name) {
            problems.push(SchemaProblem::DecideTwice { book });
            continue;
        }
        rules.insert(name, read_rule(rule, &place, &declared, problems));
    }
    let shelf = decl
        .shelf
        .map(|rule| read_rule(rule, &RulePlace::Shelf, &declared, problems));
    if ruled_books && shelf.is_none() {
        problems.push(SchemaProblem::NoShelfRule);
    }
    Authority {
        groups,
        books: rules,
        shelf,
    }
}

fn read_group(decl: GroupDecl, problems: &mut Vec<SchemaProblem>) -> Option<Group> {
    let name = match GroupName::new(&decl.name) {
        Ok(name) => Some(name),
        Err(why) => {
            problems.push(SchemaProblem::BadName {
                place: NamePlace::Group,
                text: decl.name.clone(),
                why,
            });
            None
        }
    };
    let written = decl.members.len();
    let mut members = Vec::new();
    for member in decl.members {
        match Actor::new(&member.name) {
            Err(source) => problems.push(SchemaProblem::BadMember {
                group: decl.name.clone(),
                member: member.name,
                source,
            }),
            Ok(actor) => {
                if let (Some(from), Some(until)) = (member.from, member.until)
                    && from >= until
                {
                    problems.push(SchemaProblem::MemberSpan {
                        group: decl.name.clone(),
                        member: actor,
                    });
                    continue;
                }
                members.push(Member {
                    name: actor,
                    from: member.from,
                    until: member.until,
                });
            }
        }
    }
    if written == 0 {
        problems.push(SchemaProblem::EmptyGroup { group: decl.name });
        return None;
    }
    // A group whose members are all refused has its problems told already.
    Some(Group {
        name: name?,
        members: NonEmptyVec::from_vec(members).ok()?,
    })
}

/// The groups a rule may name: those read, and every name written for one.
struct Groups<'a> {
    read: &'a BTreeMap<GroupName, Group>,
    written: &'a BTreeSet<String>,
}

fn read_rule(
    decl: RuleDecl,
    place: &RulePlace,
    groups: &Groups<'_>,
    problems: &mut Vec<SchemaProblem>,
) -> DecideRule {
    let mut names = |kind: Option<DecideKind>, written: Vec<String>| {
        if written.is_empty() {
            problems.push(SchemaProblem::NoGroup {
                at: place.clone(),
                kind,
            });
            return None;
        }
        let mut named = BTreeSet::new();
        for group in written {
            match GroupName::new(&group) {
                Ok(name) if groups.read.contains_key(&name) => {
                    named.insert(name);
                }
                // A group with problems of its own, told already.
                _ if groups.written.contains(&group) => {}
                _ => problems.push(SchemaProblem::UnknownGroup {
                    at: place.clone(),
                    group,
                }),
            }
        }
        NonEmptySet::from_set(named).ok()
    };
    let mut kinds = BTreeMap::new();
    for (kind, written) in decl.kinds {
        if let Some(set) = names(Some(kind), written) {
            kinds.insert(kind, set);
        }
    }
    let default = decl.default.and_then(|written| names(None, written));
    DecideRule {
        kinds,
        default,
        since: decl.since,
    }
}
