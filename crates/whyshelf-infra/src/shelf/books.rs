//! The declaration of the books: `.whyshelf/books.toml`.
//!
//! ```toml
//! [[book]]
//! name = "finding"
//! dir = "docs/findings"
//! key = { prefix = "F" }          # keys F-1, F-2, F-name; or key = "slug"
//! include = ["**/*.md"]            # optional: globs relative to dir; default every .md
//! exclude = ["archive/**"]         # optional: globs relative to dir; default none
//! without_front_matter = "skip"    # optional: "invalid" (default) or "skip"
//! description = "what was seen"    # optional: one line, not blank; `whyshelf map` shows it
//!
//! [[book.field]]
//! name = "severity"
//! type = { enum = ["low", "high"] } # or "text", "line", "integer", "bool", "list"
//! required = true                  # default false; a blank text or an empty list counts as none
//!
//! [[book]]
//! name = "spec"
//! dir = "docs/specs"
//! key = { prefix = "S" }
//!
//! [[book.link]]
//! name = "grounds"
//! target = "finding"
//! count = "at-least-one"           # "one", "optional", "many" (default), "at-least-one"
//! means = "depends"                # "depends", "informs", "replaces"
//! cover = "warn"                   # "error" (or true), "warn", false (default)
//!
//! [[book.link]]
//! name = "tests"
//! target = "artifact"              # things outside the documents, by name in a space
//! space = "file"                   # the space: one an adapter offers
//! means = "depends"                # "depends" or "informs" (an artifact replaces nothing)
//! compare = "lines"                # "bytes" (default), "lines" or "presence"
//! ignore = ["//"]                  # for "lines": the lines it leaves out
//!
//! [[book.link]]
//! name = "code"
//! target = "artifact"
//! preset = "rust"                  # the values of a preset; each value written here wins
//!
//! [[preset]]                       # a preset of the project
//! name = "rust"
//! from = "file"                    # optional: a built-in preset or another of the project
//! compare = "lines"                # any of space, compare, ignore and means
//! ignore = ["//"]
//!
//! [defaults]
//! artifact = "file"                # the preset of a link to artifacts that names
//!                                  # neither a preset nor a space; none when not written
//!
//! [[group]]                        # optional: names of actors, for who may decide
//! name = "maintainers"
//! members = ["ana", { name = "bo", until = "2026-12-01" }]   # also `from`
//!
//! [decide]                         # who may decide about the records of no book
//! default = ["maintainers"]
//! approvals = 2                    # optional: at least the most a book asks for
//! ```
//!
//! and, in a `[[book]]`, who may decide about its documents:
//!
//! ```toml
//! [book.decide]
//! approve = ["maintainers"]        # any of approve, deny, deprecate, retire, revoke,
//!                                  # same, exempt, moved
//! default = ["maintainers"]        # the kinds not named
//! since = "2026-12-01"             # optional: the records made from then on
//! approvals = 2                    # optional: different names to approve the same content
//! ```
//!
//! `approvals` is read as a whole number as written; one that is no number is a
//! failure to read the TOML, and the domain says whether it is in its range.
//!
//! What a group and a rule mean, and that the rule is about the names written, with
//! no authentication, is on the page of
//! [`Authority`](whyshelf_core::domain::shelf::schema::Authority)'s module. A time
//! (`since`, `from`, `until`) is read as a time typed on the command line is
//! ([`parse_time`]), and one that is neither form is a
//! failure to read the TOML, as a key a rule does not know is.
//!
//! The word `artifact` as a target is read here into
//! [`LinkTargetDecl::Artifact`]; the domain refuses a book of that name, so the word
//! never names both. `space` is read as written onto the link, and the domain says
//! whether it is one the adapters offer. `means` may be left out of a link when the
//! preset it names, or the default, gives it.
//!
//! The words of `type`, `count` and `means` are kept as written, and the domain
//! reads them: those of `type` are the words of
//! [`ScalarType`](whyshelf_core::domain::shelf::schema::ScalarType) (an enum is
//! written as a table, as above), those of `count` the words of
//! [`Cardinality`](whyshelf_core::domain::shelf::schema::Cardinality) (a link that
//! writes none has
//! [`WHEN_UNWRITTEN`](whyshelf_core::domain::shelf::schema::Cardinality::WHEN_UNWRITTEN)),
//! and those of `means` the words of
//! [`Propagation`](whyshelf_core::domain::shelf::schema::Propagation), on a link and
//! on a preset alike. A word that is none of them is a problem of the declaration
//! ([`SchemaProblem::UnknownWord`](whyshelf_core::domain::shelf::schema::SchemaProblem::UnknownWord)),
//! not of the TOML.
//!
//! The words of the entries of a book are kept as written too: a `key` written as a
//! text and a `without_front_matter`. The domain reads them, and a word that is none
//! of them is a problem of the declaration
//! ([`SchemaProblem::UnknownBookWord`](whyshelf_core::domain::shelf::schema::SchemaProblem::UnknownBookWord)).
//! A `key` that is neither a text nor a table (`key = 3`), and a table that holds an
//! entry other than `prefix`, are failures to read the TOML.
//!
//! The built-in presets are the
//! adapters' (`file`, on the page of [`artifact`](super::artifact)); how a preset of
//! the project of the same name stands to one is stated on
//! [`expand`](whyshelf_core::domain::shelf::schema::expand).
//!
//! This module reads the text and its shape; whether the books make sense (names,
//! directories that overlap, links to a book that is not declared, presets that are
//! not there) is
//! [`Schema::declared`](whyshelf_core::domain::shelf::schema::Schema::declared)'s
//! work, which [`expand`](whyshelf_core::domain::shelf::schema::expand)s the presets
//! first. Fields, links and presets stay lists, so a name written twice reaches the
//! domain twice and is reported there.

use std::io;
use std::path::{Path, PathBuf};

use super::create;
use super::system::parse_time;
use serde::Deserialize;
use whyshelf_core::app::shelf::{BookSchemaSource, Creation, DeclarationText, DeclarationWrite};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::schema::{
    ARTIFACT_TARGET, BookRuleDecl, BookSchemaDecl, CoverDecl, DecideDecl, DecideKind, DefaultsDecl,
    FieldEntryDecl, FieldPresence, FieldTypeDecl, GroupDecl, KeyPatternDecl, LinkTargetDecl,
    MemberDecl, PresetDecl, RuleDecl, RuleTime, ScopeDecl, ShelfDecl, WrittenBookDecl,
    WrittenLinkDecl, WrittenPresetValues,
};

/// Why the declaration of the books could not be read.
#[derive(Debug, thiserror::Error)]
pub enum BooksError {
    /// There is no declaration. A project with no `books.toml` is far more likely a
    /// wrong directory than a shelf with no book.
    #[error("{} is not there: declare the books of the project in it", .path.display())]
    Missing {
        /// The file looked for.
        path: PathBuf,
    },
    /// The file could not be read.
    #[error("cannot read {}", .path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// The text is not TOML of the shape above. The parser's message says where.
    #[error("{} is not a declaration of books", .path.display())]
    Shape {
        /// The file (for a text not read from it, the file it would be written to).
        path: PathBuf,
        /// What the parser said.
        #[source]
        source: toml::de::Error,
    },
    /// A step of writing the file failed: what it was, with the path and what
    /// became of the file it made.
    #[error("cannot {step}")]
    Write {
        /// The step, with its path.
        step: String,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
}

/// The declaration of the books, read from a TOML file, and written once, where none
/// is, when a project is started ([`DeclarationWrite`]).
#[derive(Debug, Clone)]
pub struct BooksToml {
    root: PathBuf,
    file: RelPath,
    path: PathBuf,
}

impl BooksToml {
    /// The declaration in the file `file` of the project at `root`.
    pub fn new(root: &Path, file: RelPath) -> Self {
        Self {
            root: root.to_path_buf(),
            path: root.join(file.as_str()),
            file,
        }
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The file is created only where nothing is (`create_new`), with its directory, and
/// the text is written as it is, comments and all; a write that fails removes the
/// file it made, and only it.
impl DeclarationWrite for BooksToml {
    type Error = BooksError;

    fn file(&self) -> RelPath {
        self.file.clone()
    }

    async fn in_the_way(&self) -> Result<Option<RelPath>, BooksError> {
        create::in_the_way(&self.root, &self.file)
            .await
            .map_err(|source| BooksError::Io {
                path: self.path.clone(),
                source,
            })
    }

    async fn create(&self, text: &DeclarationText) -> Result<Creation, BooksError> {
        create::create_new(&self.path, text.as_str().as_bytes())
            .await
            .map_err(|failed| BooksError::Write {
                step: failed.step,
                source: failed.source,
            })
    }
}

impl BookSchemaSource for BooksToml {
    type Error = BooksError;

    fn declarations_in(&self, text: &DeclarationText) -> Result<ShelfDecl, BooksError> {
        parse(text.as_str()).map_err(|source| BooksError::Shape {
            path: self.path.clone(),
            source,
        })
    }

    async fn declarations(&self) -> Result<ShelfDecl, BooksError> {
        let text = match tokio::fs::read_to_string(&self.path).await {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(BooksError::Missing {
                    path: self.path.clone(),
                });
            }
            Err(source) => {
                return Err(BooksError::Io {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        self.declarations_in(&DeclarationText::new(text))
    }
}

/// The declaration a TOML text holds: its books, presets, defaults and who may
/// decide, as written.
pub fn parse(text: &str) -> Result<ShelfDecl, toml::de::Error> {
    let file: FileDto = toml::from_str(text)?;
    let mut rules = Vec::new();
    let mut books = Vec::with_capacity(file.book.len());
    for mut book in file.book {
        if let Some(rule) = book.decide.take() {
            rules.push(BookRuleDecl {
                book: book.name.clone(),
                rule: rule.into_decl(),
            });
        }
        books.push(book.into_decl());
    }
    Ok(ShelfDecl {
        books,
        presets: file.preset.into_iter().map(PresetDto::into_decl).collect(),
        defaults: DefaultsDecl {
            artifact: file.defaults.and_then(|d| d.artifact),
        },
        decide: DecideDecl {
            groups: file.group.into_iter().map(GroupDto::into_decl).collect(),
            shelf: file.decide.map(RuleDto::into_decl),
            books: rules,
        },
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDto {
    #[serde(default)]
    book: Vec<BookDto>,
    #[serde(default)]
    preset: Vec<PresetDto>,
    defaults: Option<DefaultsDto>,
    #[serde(default)]
    group: Vec<GroupDto>,
    /// The rule of the records of no book.
    decide: Option<RuleDto>,
}

/// A group of actors: a name and its members, each a name or a table with the times
/// it is in the group.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupDto {
    name: String,
    members: Vec<MemberDto>,
}

impl GroupDto {
    fn into_decl(self) -> GroupDecl {
        GroupDecl {
            name: self.name,
            members: self
                .members
                .into_iter()
                .map(|member| match member {
                    MemberDto::Name(name) => MemberDecl {
                        name,
                        from: None,
                        until: None,
                    },
                    MemberDto::Spanned(SpannedDto { name, from, until }) => {
                        MemberDecl { name, from, until }
                    }
                })
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(
    untagged,
    expecting = "a member is a name, or a table { name, from, until } whose times are RFC 3339 with an offset or `Z`, or a date `YYYY-MM-DD`"
)]
enum MemberDto {
    Name(String),
    Spanned(SpannedDto),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpannedDto {
    name: String,
    #[serde(default, deserialize_with = "time")]
    from: Option<RuleTime>,
    #[serde(default, deserialize_with = "time")]
    until: Option<RuleTime>,
}

/// A rule of who may decide: the groups of each kind of judgement it names, of the
/// kinds it does not, and from when it reads records. A key that names no kind is
/// refused here.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDto {
    approve: Option<Vec<String>>,
    deny: Option<Vec<String>>,
    deprecate: Option<Vec<String>>,
    retire: Option<Vec<String>>,
    revoke: Option<Vec<String>>,
    same: Option<Vec<String>>,
    exempt: Option<Vec<String>>,
    moved: Option<Vec<String>>,
    default: Option<Vec<String>>,
    #[serde(default, deserialize_with = "time")]
    since: Option<RuleTime>,
    /// As written: the domain says whether it is in range.
    approvals: Option<i64>,
}

impl RuleDto {
    fn into_decl(self) -> RuleDecl {
        let kinds = [
            (DecideKind::Approve, self.approve),
            (DecideKind::Deny, self.deny),
            (DecideKind::Deprecate, self.deprecate),
            (DecideKind::Retire, self.retire),
            (DecideKind::Revoke, self.revoke),
            (DecideKind::Same, self.same),
            (DecideKind::Exempt, self.exempt),
            (DecideKind::Moved, self.moved),
        ];
        RuleDecl {
            kinds: kinds
                .into_iter()
                .filter_map(|(kind, groups)| groups.map(|groups| (kind, groups)))
                .collect(),
            default: self.default,
            since: self.since,
            approvals: self.approvals,
        }
    }
}

/// A time of a rule as written: a string, or a TOML date or date-time, which is
/// read as its text.
#[derive(Deserialize)]
#[serde(untagged)]
enum TimeDto {
    Text(String),
    Toml(toml::value::Datetime),
}

/// A time of a rule, read as a time typed on the command line is
/// ([`parse_time`]), whether quoted or a TOML date (`since = 2026-12-01`); one that
/// is neither form (a date-time with no offset, say) is a failure to read the TOML.
fn time<'de, D>(deserializer: D) -> Result<Option<RuleTime>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let written: Option<TimeDto> = Option::deserialize(deserializer)?;
    written
        .map(|written| {
            let text = match written {
                TimeDto::Text(text) => text,
                TimeDto::Toml(at) => at.to_string(),
            };
            parse_time(&text)
                .map(|at| RuleTime::from_millis(at.millis()))
                .map_err(serde::de::Error::custom)
        })
        .transpose()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultsDto {
    /// The preset of a link to artifacts that names neither a preset nor a space.
    artifact: Option<String>,
}

/// A preset of the project: a name, the preset it comes from, and the values of a
/// link to artifacts it gives. Any other key is refused here, as on a link.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetDto {
    name: String,
    from: Option<String>,
    space: Option<String>,
    compare: Option<String>,
    ignore: Option<Vec<String>>,
    /// The word as written, read by the domain with the words of a link, which
    /// refuses `replaces`.
    means: Option<String>,
}

impl PresetDto {
    fn into_decl(self) -> PresetDecl {
        PresetDecl {
            name: self.name,
            from: self.from,
            values: WrittenPresetValues {
                space: self.space,
                compare: self.compare,
                ignore: self.ignore,
                means: self.means,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BookDto {
    name: String,
    dir: String,
    /// Glob patterns relative to `dir`: the files that are documents. Not written:
    /// every `.md` file. Checked by the domain.
    include: Option<Vec<String>>,
    /// Glob patterns relative to `dir`: files that are not documents. Checked by the
    /// domain.
    exclude: Option<Vec<String>>,
    /// `"invalid"` (default) or `"skip"`: what a file in range with no front matter
    /// is. Checked by the domain.
    without_front_matter: Option<String>,
    key: KeyDto,
    /// What the book is for, one line; checked by the domain.
    description: Option<String>,
    #[serde(default)]
    field: Vec<FieldDto>,
    #[serde(default)]
    link: Vec<LinkDto>,
    /// Who may decide about the documents of the book.
    decide: Option<RuleDto>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum KeyDto {
    Prefixed(PrefixDto),
    /// A text, kept as written: the domain reads it.
    Word(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrefixDto {
    prefix: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldDto {
    name: String,
    #[serde(rename = "type")]
    ty: TypeDto,
    #[serde(default)]
    required: bool,
}

/// A word as written, read by the domain, or the table of an enum.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TypeDto {
    Word(String),
    Enum(EnumDto),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnumDto {
    #[serde(rename = "enum")]
    words: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkDto {
    name: String,
    target: String,
    /// The word as written; the domain reads it, and takes the count of a link that
    /// writes none.
    count: Option<String>,
    /// The word as written. May be left out when the preset or the default gives
    /// it; a link left with none is a problem of the declaration, which the domain
    /// tells.
    means: Option<String>,
    /// For a link to artifacts: the preset whose values it takes; checked by the
    /// domain.
    preset: Option<String>,
    /// For a link to artifacts: the space its names are in; checked by the domain.
    space: Option<String>,
    /// For a link to artifacts: `bytes`, `lines` or `presence`; checked by the domain.
    compare: Option<String>,
    /// For `compare = "lines"`: the prefixes of the lines left out.
    ignore: Option<Vec<String>>,
    /// For a link into a book: how strongly every document of the target book is
    /// expected to be reached by it, a truth value or a word; checked by the domain.
    cover: Option<CoverDto>,
}

#[derive(Debug, Deserialize)]
#[serde(
    untagged,
    expecting = "`cover` takes true, false, \"error\" or \"warn\""
)]
enum CoverDto {
    Truth(bool),
    Word(String),
}

impl BookDto {
    fn into_decl(self) -> WrittenBookDecl {
        BookSchemaDecl {
            name: self.name,
            dir: self.dir,
            scope: ScopeDecl {
                include: self.include,
                exclude: self.exclude,
                without_front_matter: self.without_front_matter,
            },
            key: match self.key {
                KeyDto::Prefixed(PrefixDto { prefix }) => KeyPatternDecl::Prefixed { prefix },
                KeyDto::Word(word) => KeyPatternDecl::Word(word),
            },
            description: self.description,
            fields: self
                .field
                .into_iter()
                .map(|f| FieldEntryDecl {
                    name: f.name,
                    ty: match f.ty {
                        TypeDto::Word(word) => FieldTypeDecl::Word(word),
                        TypeDto::Enum(EnumDto { words }) => FieldTypeDecl::Enum(words),
                    },
                    presence: if f.required {
                        FieldPresence::Required
                    } else {
                        FieldPresence::Optional
                    },
                })
                .collect(),
            links: self
                .link
                .into_iter()
                .map(|l| WrittenLinkDecl {
                    name: l.name,
                    target: if l.target == ARTIFACT_TARGET {
                        LinkTargetDecl::Artifact
                    } else {
                        LinkTargetDecl::Book(l.target)
                    },
                    count: l.count,
                    means: l.means,
                    preset: l.preset,
                    space: l.space,
                    compare: l.compare,
                    ignore: l.ignore,
                    cover: l.cover.map(|cover| match cover {
                        CoverDto::Truth(truth) => CoverDecl::Truth(truth),
                        CoverDto::Word(word) => CoverDecl::Word(word),
                    }),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use whyshelf_core::domain::shelf::artifact::Space;
    use whyshelf_core::domain::shelf::schema::{
        Approvals, Cardinality, Propagation, Schema, SchemaProblem, WordAt, WordOf,
    };

    use super::*;

    const SAMPLE: &str = r#"
[[book]]
name = "finding"
dir = "docs/findings"
key = { prefix = "F" }

[[book.field]]
name = "severity"
type = { enum = ["low", "high"] }
required = true

[[book]]
name = "note"
dir = "docs/notes"
key = "slug"

[[book.link]]
name = "grounds"
target = "finding"
count = "at-least-one"
means = "depends"

[[book.link]]
name = "see"
target = "finding"
means = "informs"
"#;

    #[test]
    fn a_declaration_is_read_into_the_values_of_the_domain() {
        let decls = parse(SAMPLE).unwrap();
        assert_eq!(decls.books.len(), 2);
        assert_eq!(decls.books[0].name, "finding");
        assert_eq!(
            decls.books[0].key,
            KeyPatternDecl::Prefixed {
                prefix: "F".to_owned()
            }
        );
        assert_eq!(
            decls.books[0].fields[0].ty,
            FieldTypeDecl::Enum(vec!["low".to_owned(), "high".to_owned()])
        );
        assert_eq!(decls.books[0].fields[0].presence, FieldPresence::Required);
        assert_eq!(decls.books[1].key, KeyPatternDecl::slug());
        // The words are kept as written; a link that writes no count has none.
        assert_eq!(
            decls.books[1].links[0].count.as_deref(),
            Some("at-least-one")
        );
        assert_eq!(decls.books[1].links[0].means.as_deref(), Some("depends"));
        assert_eq!(decls.books[1].links[1].count, None);
        assert_eq!(decls.books[1].links[1].means.as_deref(), Some("informs"));
        let schema = validate(decls).unwrap();
        let book = schema.book("note").unwrap();
        let cardinalities: Vec<(String, Cardinality)> = book
            .links()
            .iter()
            .map(|(name, kind)| (name.to_string(), kind.cardinality))
            .collect();
        assert_eq!(
            cardinalities,
            [
                ("grounds".to_owned(), Cardinality::AtLeastOne),
                ("see".to_owned(), Cardinality::WHEN_UNWRITTEN),
            ]
        );
        assert_eq!(Cardinality::WHEN_UNWRITTEN, Cardinality::Many);
    }

    #[test]
    fn a_word_the_domain_does_not_know_is_a_problem_of_the_declaration_and_not_of_the_toml() {
        let decls = parse(
            "[[preset]]\nname='p'\nmeans='x'\n\
             [[book]]\nname='note'\ndir='n'\nkey='slug'\n\
             [[book.field]]\nname='n'\ntype='number'\n\
             [[book.link]]\nname='grounds'\ntarget='note'\ncount='several'\nmeans='leans'\n",
        )
        .expect("the TOML is of the shape; its words are the domain's to read");
        let problems = validate(decls).unwrap_err().into_vec();
        let at = |at: WordAt, word: &str, of: WordOf| SchemaProblem::UnknownWord {
            at,
            word: word.to_owned(),
            of,
        };
        let link = || WordAt::Link {
            book: "note".to_owned(),
            link: "grounds".to_owned(),
        };
        assert_eq!(
            problems,
            [
                at(
                    WordAt::Preset {
                        name: "p".to_owned()
                    },
                    "x",
                    WordOf::Means
                ),
                at(link(), "several", WordOf::Count),
                at(link(), "leans", WordOf::Means),
                at(
                    WordAt::Field {
                        book: "note".to_owned(),
                        field: "n".to_owned()
                    },
                    "number",
                    WordOf::Type
                ),
            ]
        );
        assert_eq!(
            problems[1].to_string(),
            "book `note`: the count of the link `grounds` is \"several\", and it takes one of \
             \"one\", \"optional\", \"many\", \"at-least-one\""
        );
    }

    /// The spaces the tests hold declarations against.
    fn spaces() -> BTreeSet<Space> {
        BTreeSet::from([Space::new("file").unwrap()])
    }

    /// The declaration expanded with the presets of the file adapter, then checked.
    #[allow(clippy::result_large_err)]
    fn validate(
        decls: ShelfDecl,
    ) -> Result<Schema, whyshelf_core::domain::nonempty::NonEmptyVec<SchemaProblem>> {
        let builtins = crate::shelf::artifact::file::presets();
        Schema::declared(decls, &builtins, &spaces())
    }

    #[test]
    fn the_target_artifact_is_a_link_to_artifacts_and_not_to_a_book() {
        let decls = parse(
            "[[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [[book.link]]\nname='tests'\ntarget='artifact'\nspace='file'\nmeans='depends'\n",
        )
        .unwrap();
        assert_eq!(decls.books[0].links[0].target, LinkTargetDecl::Artifact);
        assert_eq!(decls.books[0].links[0].space.as_deref(), Some("file"));
        let schema = validate(decls).unwrap();
        let links = schema.book("a").unwrap().artifact_links();
        assert_eq!(links.len(), 1);
        assert_eq!(links.get("tests").unwrap().space.as_str(), "file");
    }

    #[test]
    fn a_link_to_artifacts_without_a_space_is_read_as_none_and_refused() {
        let decls = parse(
            "[[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [[book.link]]\nname='tests'\ntarget='artifact'\nmeans='depends'\n",
        )
        .unwrap();
        assert_eq!(decls.books[0].links[0].space, None);
        let problems = validate(decls).unwrap_err();
        assert!(matches!(
            problems.first(),
            SchemaProblem::NoSpace { link, .. } if link.as_str() == "tests"
        ));
    }

    #[test]
    fn a_key_the_shape_does_not_know_is_refused() {
        assert!(parse("[[book]]\nname='a'\ndir='a'\nkey='slug'\ncolour='red'\n").is_err());
        // Neither a text nor a table: no shape of a key.
        assert!(parse("[[book]]\nname='a'\ndir='a'\nkey=3\n").is_err());
        assert!(parse("[[book]]\nname='a'\ndir='a'\nkey=[]\n").is_err());
        // A table with a key it does not know.
        assert!(parse("[[book]]\nname='a'\ndir='a'\nkey={prefix='F',colour='red'}\n").is_err());
    }

    #[test]
    fn a_word_of_a_key_is_kept_as_written_for_the_domain_to_read() {
        let decls = parse("[[book]]\nname='a'\ndir='a'\nkey='uuid'\n").unwrap();
        assert_eq!(decls.books[0].key, KeyPatternDecl::Word("uuid".to_owned()));
        let problems = validate(decls).unwrap_err();
        assert!(matches!(
            problems.first(),
            SchemaProblem::UnknownBookWord { book, word, .. } if book == "a" && word == "uuid"
        ));
    }

    /// A book `a` whose link `tests` to artifacts has these extra lines.
    fn tests_link(extra: &str) -> String {
        format!(
            "[[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [[book.link]]\nname='tests'\ntarget='artifact'\nspace='file'\nmeans='depends'\n{extra}"
        )
    }

    #[test]
    fn compare_and_ignore_are_read_onto_a_link_and_none_is_no_compare() {
        let decls = parse(&tests_link("compare='lines'\nignore=['//', '#']\n")).unwrap();
        assert_eq!(decls.books[0].links[0].compare.as_deref(), Some("lines"));
        assert_eq!(
            decls.books[0].links[0].ignore,
            Some(vec!["//".to_owned(), "#".to_owned()])
        );
        let schema = validate(decls).unwrap();
        let kind = schema
            .book("a")
            .unwrap()
            .artifact_links()
            .get("tests")
            .unwrap();
        assert_eq!(kind.comparison.kind_word(), "lines");
        assert_eq!(
            kind.comparison
                .ignored()
                .map(|p| p.as_str())
                .collect::<Vec<_>>(),
            ["#", "//"]
        );

        let plain = parse(&tests_link("")).unwrap();
        assert_eq!(plain.books[0].links[0].compare, None);
        assert_eq!(plain.books[0].links[0].ignore, None);
        let bytes = parse(&tests_link("compare='bytes'\n")).unwrap();
        assert_eq!(bytes.books[0].links[0].compare.as_deref(), Some("bytes"));
        assert_eq!(bytes.books[0].links[0].ignore, None);
    }

    #[test]
    fn ignore_without_compare_is_kept_for_the_domain_to_refuse() {
        let decls = parse(&tests_link("ignore=['//']\n")).unwrap();
        assert_eq!(decls.books[0].links[0].compare, None);
        assert_eq!(decls.books[0].links[0].ignore, Some(vec!["//".to_owned()]));
        assert!(validate(decls).is_err());
    }

    #[test]
    fn presets_and_defaults_are_read_as_written() {
        let decls = parse(
            "[defaults]\nartifact='rust'\n\
             [[preset]]\nname='rust'\nfrom='file'\ncompare='lines'\nignore=['//']\nmeans='informs'\n\
             [[preset]]\nname='bare'\n\
             [[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [[book.link]]\nname='code'\ntarget='artifact'\npreset='bare'\nspace='file'\n",
        )
        .unwrap();
        assert_eq!(decls.defaults.artifact.as_deref(), Some("rust"));
        assert_eq!(
            decls.presets,
            [
                PresetDecl {
                    name: "rust".to_owned(),
                    from: Some("file".to_owned()),
                    values: WrittenPresetValues {
                        space: None,
                        compare: Some("lines".to_owned()),
                        ignore: Some(vec!["//".to_owned()]),
                        means: Some("informs".to_owned()),
                    },
                },
                PresetDecl {
                    name: "bare".to_owned(),
                    from: None,
                    values: WrittenPresetValues::default(),
                },
            ]
        );
        let link = &decls.books[0].links[0];
        assert_eq!(link.preset.as_deref(), Some("bare"));
        assert_eq!(link.means, None);
        // `bare` gives no meaning, so the link that names it says none.
        let problems = validate(decls).unwrap_err().into_vec();
        assert!(
            matches!(&problems[..], [SchemaProblem::NoMeans { link, .. }] if link == "code"),
            "{problems:?}"
        );
        assert!(parse("").unwrap().defaults.artifact.is_none());
    }

    #[test]
    fn a_default_of_the_file_preset_expands_a_link_to_the_space_file() {
        let decls = parse(
            "[defaults]\nartifact='file'\n\
             [[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [[book.link]]\nname='tests'\ntarget='artifact'\n",
        )
        .unwrap();
        let schema = validate(decls).unwrap();
        let book = schema.book("a").unwrap();
        let kind = book.artifact_links().get("tests").unwrap();
        assert_eq!(kind.space.as_str(), "file");
        assert_eq!(kind.comparison.kind_word(), "bytes");
        assert_eq!(kind.propagation.propagation(), Propagation::Depends);
        assert!(book.preset_of("tests").is_some_and(|p| p.is_default()));
    }

    #[test]
    fn a_key_a_preset_or_the_defaults_do_not_know_is_refused() {
        assert!(parse("[[preset]]\nname='p'\ncount='one'\n").is_err());
        assert!(parse("[[preset]]\nname='p'\ncover=true\n").is_err());
        assert!(parse("[defaults]\nbook='p'\n").is_err());
    }

    #[test]
    fn a_key_a_link_does_not_know_is_still_refused() {
        assert!(parse(&tests_link("compare='lines'\nskip=['//']\n")).is_err());
        assert!(parse(&tests_link("compare=['lines']\n")).is_err());
    }

    /// A book `a` with a link `up` into itself, with `extra` lines on the link.
    fn book_link(extra: &str) -> String {
        format!(
            "[[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [[book.link]]\nname='up'\ntarget='a'\nmeans='depends'\n{extra}"
        )
    }

    #[test]
    fn cover_is_read_onto_a_link_as_a_truth_value_or_a_word() {
        for (text, read, strength) in [
            (
                "cover = true\n",
                Some(CoverDecl::Truth(true)),
                Some("error"),
            ),
            ("cover = false\n", Some(CoverDecl::Truth(false)), None),
            (
                "cover = 'error'\n",
                Some(CoverDecl::Word("error".to_owned())),
                Some("error"),
            ),
            (
                "cover = 'warn'\n",
                Some(CoverDecl::Word("warn".to_owned())),
                Some("warn"),
            ),
            ("", None, None),
        ] {
            let decls = parse(&book_link(text)).unwrap();
            assert_eq!(decls.books[0].links[0].cover, read, "{text}");
            let schema = validate(decls).unwrap();
            let book = schema.book("a").unwrap();
            let (_, kind) = book
                .links()
                .iter()
                .find(|(name, _)| name.as_str() == "up")
                .unwrap();
            assert_eq!(kind.cover.map(|c| c.word()), strength, "{text}");
        }
    }

    #[test]
    fn a_cover_that_is_neither_a_truth_value_nor_a_word_is_refused() {
        for value in ["1", "['yes']"] {
            let error = parse(&book_link(&format!("cover = {value}\n")))
                .expect_err(value)
                .to_string();
            assert!(
                error.contains(r#"true, false, "error" or "warn""#),
                "cover = {value}: {error}"
            );
        }
        // A word is read as written; the domain refuses one it does not know.
        let decls = parse(&book_link("cover = 'true'\n")).unwrap();
        let problems = validate(decls).unwrap_err().into_vec();
        assert!(
            matches!(&problems[..], [SchemaProblem::BadCover { value, .. }] if value == "true"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_description_is_read_as_written_and_none_when_not_written() {
        let decls = parse("[[book]]\nname='a'\ndir='a'\nkey='slug'\ndescription='what was seen'\n")
            .unwrap();
        assert_eq!(decls.books[0].description.as_deref(), Some("what was seen"));
        let schema = validate(decls).unwrap();
        assert_eq!(
            schema.book("a").unwrap().description().map(|d| d.as_str()),
            Some("what was seen")
        );
        assert_eq!(
            parse("[[book]]\nname='a'\ndir='a'\nkey='slug'\n")
                .unwrap()
                .books[0]
                .description,
            None
        );
        let two_lines =
            parse("[[book]]\nname='a'\ndir='a'\nkey='slug'\ndescription=\"\"\"\none\ntwo\"\"\"\n")
                .unwrap();
        assert!(matches!(
            validate(two_lines).unwrap_err().first(),
            SchemaProblem::BadDescription { .. }
        ));
        assert!(parse("[[book]]\nname='a'\ndir='a'\nkey='slug'\ndescription=1\n").is_err());
    }

    #[test]
    fn who_may_decide_is_read_into_the_authority_of_the_schema() {
        let decls = parse(
            "[[group]]\nname='maintainers'\nmembers=['ana', {name='bo', until='2026-12-01'}]\n\
             [decide]\ndefault=['maintainers']\n\
             [[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [book.decide]\napprove=['maintainers']\nsince='2026-11-11'\n",
        )
        .unwrap();
        assert_eq!(decls.decide.groups[0].members[1].name, "bo");
        assert!(decls.decide.groups[0].members[1].until.is_some());
        assert_eq!(decls.decide.books[0].book, "a");
        assert_eq!(
            decls.decide.books[0].rule.since.map(|t| t.millis()),
            Some(1_794_355_200_000)
        );
        let schema = validate(decls).unwrap();
        let authority = schema.authority();
        assert!(!authority.is_unruled());
        let book = schema.book("a").unwrap().name().clone();
        let rule = authority.rule_of(Some(&book)).unwrap();
        assert!(rule.named(DecideKind::Approve).is_some());
        assert!(authority.rule_of(None).is_some());
        // A project that declares none is unruled.
        let plain = validate(parse("[[book]]\nname='a'\ndir='a'\nkey='slug'\n").unwrap()).unwrap();
        assert!(plain.authority().is_unruled());
    }

    #[test]
    fn a_time_of_a_rule_may_be_a_toml_date_but_not_one_with_no_offset() {
        let rule = |since: &str| {
            parse(&format!(
                "[[group]]\nname='m'\nmembers=['ana']\n[decide]\ndefault=['m']\nsince={since}\n"
            ))
        };
        let quoted = rule("'2026-11-11'").unwrap();
        let date = rule("2026-11-11").unwrap();
        let instant = rule("2026-11-11T00:00:00Z").unwrap();
        let millis = |d: &ShelfDecl| {
            d.decide
                .shelf
                .as_ref()
                .and_then(|r| r.since)
                .map(|t| t.millis())
        };
        assert_eq!(millis(&quoted), Some(1_794_355_200_000));
        assert_eq!(millis(&date), millis(&quoted));
        assert_eq!(millis(&instant), millis(&quoted));
        assert!(rule("2026-11-11T00:00:00").is_err());
    }

    #[test]
    fn a_rule_of_who_may_decide_that_cannot_be_used_is_a_problem_of_the_declaration() {
        let decls = parse(
            "[[group]]\nname='empty'\nmembers=[]\n\
             [[group]]\nname='late'\nmembers=[{name='ana', from='2026-12-01', until='2026-11-01'}]\n\
             [[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [book.decide]\napprove=['nobody']\ndeny=[]\n",
        )
        .unwrap();
        let problems: Vec<String> = validate(decls)
            .unwrap_err()
            .into_vec()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            problems,
            [
                "the group `empty` has no member",
                "the group `late`: the `from` of `ana` is not before its `until`",
                "book `a`: `[book.decide]` names the group `nobody`, which is not declared",
                "book `a`: `[book.decide]`: `deny` names no group",
                "a book declares who may decide, and `[decide]` does not: the records of no \
                 book (a move, the same meaning of an artifact) and those whose book cannot be \
                 told would escape every rule",
            ]
        );
        // A time that is neither form, and a kind no rule has, are failures to read the
        // TOML.
        assert!(parse("[decide]\nsince='soon'\n").is_err());
        assert!(parse("[decide]\nread=['a']\n").is_err());
    }

    #[test]
    fn approvals_are_read_and_a_rule_that_cannot_meet_them_is_a_problem_of_the_declaration() {
        let decide = |shelf: &str, book: &str| {
            format!(
                "[[group]]\nname='m'\nmembers=['ana', 'bo']\n\
                 [decide]\ndefault=['m']\n{shelf}\n\
                 [[book]]\nname='a'\ndir='a'\nkey='slug'\n\
                 [[book]]\nname='b'\ndir='b'\nkey='slug'\n\
                 [book.decide]\napprove=['m']\n{book}\n"
            )
        };
        let schema = validate(parse(&decide("approvals=2", "approvals=2")).unwrap()).unwrap();
        let book = schema.book("b").unwrap().name().clone();
        let authority = schema.authority();
        let two = Approvals::new(2).unwrap();
        assert_eq!(authority.rule_of(Some(&book)).unwrap().approvals(), two);
        assert_eq!(authority.rule_of(None).unwrap().approvals(), two);
        // An unruled book, and a rule that writes none, ask for one name.
        let unruled = schema.book("a").unwrap().name().clone();
        assert_eq!(
            authority.approvals(Some(&unruled), RuleTime::from_millis(0)),
            Approvals::ONE
        );
        let plain = validate(parse(&decide("", "")).unwrap()).unwrap();
        let unwritten = plain
            .authority()
            .approvals(Some(&book), RuleTime::from_millis(0));
        assert_eq!(unwritten, Approvals::ONE);

        let problems = |shelf: &str, book: &str| -> Vec<String> {
            validate(parse(&decide(shelf, book)).unwrap())
                .unwrap_err()
                .into_vec()
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        assert_eq!(
            problems("approvals=3", "approvals=3"),
            [
                "book `b`: `[book.decide]` asks for 3 different names to approve, and only 2 may approve under it",
                "`[decide]` asks for 3 different names to approve, and only 2 may approve under it",
            ]
        );
        assert_eq!(
            problems("", "approvals=2"),
            [
                "`[decide]` asks for fewer different names (1) than book `b` (2): a declaration of the same meaning of an artifact keeps that book's approvals, and is ruled by `[decide]`"
            ]
        );
        assert_eq!(
            problems("approvals=11", "approvals=0"),
            [
                "book `b`: `[book.decide]`: `approvals` is 0, and it takes a number from 1 to 10",
                "`[decide]`: `approvals` is 11, and it takes a number from 1 to 10",
            ]
        );
        // A number of no whole number is a failure to read the TOML.
        assert!(parse(&decide("approvals='two'", "")).is_err());

        // `same` naming a group of fewer names than asked: no edit could ever count.
        let solo = "[[group]]\nname='m'\nmembers=['ana', 'bo']\n\
                    [[group]]\nname='s'\nmembers=['ana']\n\
                    [decide]\ndefault=['m']\napprovals=2\nsame=['s']\n";
        let problems: Vec<String> = validate(parse(solo).unwrap())
            .unwrap_err()
            .into_vec()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            problems,
            [
                "`[decide]` asks for 2 different names to declare a same meaning, and only 1 may declare a same meaning under it"
            ]
        );
        // A group that cannot be read is told once, not counted as too few names.
        let ghost = "[[group]]\nname='s'\nmembers=['ana']\n\
                     [decide]\ndefault=['s', 'ghost']\napprovals=2\n";
        let problems = validate(parse(ghost).unwrap()).unwrap_err().into_vec();
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn decide_that_reads_fewer_records_than_a_ruled_book_is_a_problem_of_the_declaration() {
        let declared = |shelf_since: &str, book_since: &str| {
            parse(&format!(
                "[[group]]\nname='m'\nmembers=['ana', 'bo']\n\
                 [decide]\ndefault=['m']\napprovals=2\n{shelf_since}\n\
                 [[book]]\nname='a'\ndir='a'\nkey='slug'\n\
                 [book.decide]\napprove=['m']\napprovals=2\n{book_since}\n"
            ))
            .unwrap()
        };
        let told = |shelf_since: &str, book_since: &str| -> Vec<String> {
            let problems = validate(declared(shelf_since, book_since)).err();
            problems.map_or_else(Vec::new, |problems| {
                problems.iter().map(ToString::to_string).collect()
            })
        };
        let later = "`[decide]` reads the records made from unix ms 1798761600000, and book `a` reads earlier ones: a declaration of the same meaning of an artifact made before then would be ruled by none, and keep that book's approvals with one name";
        // The book reads every record, `[decide]` only those from 2027.
        assert_eq!(told("since='2027-01-01'", ""), [later]);
        // The book reads from an earlier time than `[decide]`.
        assert_eq!(told("since='2027-01-01'", "since='2026-12-01'"), [later]);
        // As many records, or more: no problem.
        assert!(told("since='2027-01-01'", "since='2027-01-01'").is_empty());
        assert!(told("since='2026-12-01'", "since='2027-01-01'").is_empty());
        assert!(told("", "since='2027-01-01'").is_empty());
        assert!(told("", "").is_empty());

        // A book that asks for one name, as every book of a rule did before
        // `approvals`, puts no bound on `[decide]`'s `since`.
        let one = parse(
            "[[group]]\nname='m'\nmembers=['ana']\n\
             [decide]\ndefault=['m']\nsince='2027-01-01'\n\
             [[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [book.decide]\napprove=['m']\n",
        )
        .unwrap();
        assert!(validate(one).is_ok());
    }

    #[test]
    fn an_empty_file_declares_no_book() {
        let decls = parse("").unwrap();
        assert!(decls.books.is_empty());
        assert!(decls.presets.is_empty());
    }

    #[test]
    fn the_sample_of_init_reads_and_is_a_sound_declaration() {
        // `init::SAMPLE` is the public sample, the one `whyshelf init --template sample`
        // writes; this module has a small one of its own.
        let decls = parse(crate::shelf::init::SAMPLE).unwrap();
        let names: Vec<&str> = decls.books.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            ["finding", "requirement", "decision", "verification"]
        );
        assert_eq!(decls.defaults.artifact.as_deref(), Some("file"));
        let schema = validate(decls).unwrap();
        assert_eq!(schema.books().len(), 4);
        let verification = schema.book("verification").unwrap();
        let tests = verification.artifact_links().get("tests").unwrap();
        assert_eq!(tests.space.as_str(), "file");
        assert!(
            verification
                .preset_of("tests")
                .is_some_and(|p| p.is_default())
        );
    }

    #[tokio::test]
    async fn a_missing_file_is_an_error_and_not_an_empty_shelf() {
        let dir = tempfile::tempdir().unwrap();
        let source = BooksToml::new(dir.path(), "books.toml".parse().unwrap());
        assert!(matches!(
            source.declarations().await,
            Err(BooksError::Missing { .. })
        ));
    }

    #[tokio::test]
    async fn a_declaration_is_written_as_it_is_once_and_never_over_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let books = BooksToml::new(dir.path(), ".whyshelf/books.toml".parse().unwrap());
        assert_eq!(books.file().as_str(), ".whyshelf/books.toml");
        assert_eq!(books.in_the_way().await.unwrap(), None);
        let text = DeclarationText::new("# a comment\n[[book]]\nname='a'\ndir='a'\nkey='slug'\n");
        assert_eq!(books.create(&text).await.unwrap(), Creation::Created);
        assert_eq!(books.in_the_way().await.unwrap(), Some(books.file()));
        let written = tokio::fs::read_to_string(books.path()).await.unwrap();
        assert_eq!(written, text.as_str());
        assert_eq!(books.declarations().await.unwrap().books[0].name, "a");

        let other = DeclarationText::new("");
        assert_eq!(books.create(&other).await.unwrap(), Creation::AlreadyThere);
        let kept = tokio::fs::read_to_string(books.path()).await.unwrap();
        assert_eq!(kept, text.as_str());
    }

    #[test]
    fn a_text_is_read_by_the_reader_of_the_file() {
        let books = BooksToml::new(Path::new("/p"), "books.toml".parse().unwrap());
        assert!(matches!(
            books.declarations_in(&DeclarationText::new("[[book]]\nname=1\n")),
            Err(BooksError::Shape { .. })
        ));
        let decl = books
            .declarations_in(&DeclarationText::new(crate::shelf::init::MINIMAL))
            .unwrap();
        assert_eq!(decl.books.len(), 1);
    }
}
