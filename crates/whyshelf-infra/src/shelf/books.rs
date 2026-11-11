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
//! type = { enum = ["low", "high"] } # or "text", "integer", "bool", "list"
//! required = true                  # default false
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
//! ```
//!
//! The word `artifact` as a target is read here into
//! [`LinkTargetDecl::Artifact`]; the domain refuses a book of that name, so the word
//! never names both. `space` is read as written onto the link, and the domain says
//! whether it is one the adapters offer. `means` may be left out of a link when the
//! preset it names, or the default, gives it. The built-in presets are the
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
use serde::Deserialize;
use whyshelf_core::app::shelf::{BookSchemaSource, Creation, DeclarationText, DeclarationWrite};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::schema::{
    ARTIFACT_TARGET, BookSchemaDecl, Cardinality, CoverDecl, DefaultsDecl, FieldEntryDecl,
    FieldPresence, FieldTypeDecl, KeyPatternDecl, LinkTargetDecl, PresetDecl, PresetValues,
    Propagation, ScopeDecl, ShelfDecl, WrittenBookDecl, WrittenLinkDecl,
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

/// The declaration a TOML text holds: its books, presets and defaults, as written.
pub fn parse(text: &str) -> Result<ShelfDecl, toml::de::Error> {
    let file: FileDto = toml::from_str(text)?;
    Ok(ShelfDecl {
        books: file.book.into_iter().map(BookDto::into_decl).collect(),
        presets: file.preset.into_iter().map(PresetDto::into_decl).collect(),
        defaults: DefaultsDecl {
            artifact: file.defaults.and_then(|d| d.artifact),
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
    /// Read with the words of a link; the domain refuses `replaces`.
    means: Option<MeansWord>,
}

impl PresetDto {
    fn into_decl(self) -> PresetDecl {
        PresetDecl {
            name: self.name,
            from: self.from,
            values: PresetValues {
                space: self.space,
                compare: self.compare,
                ignore: self.ignore,
                means: self.means.map(MeansWord::propagation),
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
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum KeyDto {
    Prefixed(PrefixDto),
    Word(KeyWord),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrefixDto {
    prefix: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum KeyWord {
    Slug,
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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TypeDto {
    Word(TypeWord),
    Enum(EnumDto),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TypeWord {
    Text,
    Integer,
    Bool,
    List,
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
    #[serde(default)]
    count: CountWord,
    /// May be left out when the preset or the default gives it; a link left with
    /// none is a problem of the declaration, which the domain tells.
    means: Option<MeansWord>,
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CountWord {
    One,
    Optional,
    #[default]
    Many,
    AtLeastOne,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MeansWord {
    Depends,
    Informs,
    Replaces,
}

impl MeansWord {
    fn propagation(self) -> Propagation {
        match self {
            Self::Depends => Propagation::Depends,
            Self::Informs => Propagation::Informs,
            Self::Replaces => Propagation::Replaces,
        }
    }
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
                KeyDto::Word(KeyWord::Slug) => KeyPatternDecl::Slug,
            },
            description: self.description,
            fields: self
                .field
                .into_iter()
                .map(|f| FieldEntryDecl {
                    name: f.name,
                    ty: match f.ty {
                        TypeDto::Word(TypeWord::Text) => FieldTypeDecl::Text,
                        TypeDto::Word(TypeWord::Integer) => FieldTypeDecl::Integer,
                        TypeDto::Word(TypeWord::Bool) => FieldTypeDecl::Bool,
                        TypeDto::Word(TypeWord::List) => FieldTypeDecl::TextList,
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
                    cardinality: match l.count {
                        CountWord::One => Cardinality::One,
                        CountWord::Optional => Cardinality::Optional,
                        CountWord::Many => Cardinality::Many,
                        CountWord::AtLeastOne => Cardinality::AtLeastOne,
                    },
                    propagation: l.means.map(MeansWord::propagation),
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
    use whyshelf_core::domain::shelf::schema::{Schema, SchemaProblem};

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
        assert_eq!(decls.books[1].key, KeyPatternDecl::Slug);
        assert_eq!(decls.books[1].links[0].cardinality, Cardinality::AtLeastOne);
        assert_eq!(
            decls.books[1].links[0].propagation,
            Some(Propagation::Depends)
        );
        assert_eq!(decls.books[1].links[1].cardinality, Cardinality::Many);
        assert_eq!(
            decls.books[1].links[1].propagation,
            Some(Propagation::Informs)
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
        let builtins = crate::shelf::artifact::file::presets(&Space::new("file").unwrap());
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
        assert!(parse("[[book]]\nname='a'\ndir='a'\nkey='uuid'\n").is_err());
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
                    values: PresetValues {
                        space: None,
                        compare: Some("lines".to_owned()),
                        ignore: Some(vec!["//".to_owned()]),
                        means: Some(Propagation::Informs),
                    },
                },
                PresetDecl {
                    name: "bare".to_owned(),
                    from: None,
                    values: PresetValues::default(),
                },
            ]
        );
        let link = &decls.books[0].links[0];
        assert_eq!(link.preset.as_deref(), Some("bare"));
        assert_eq!(link.propagation, None);
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
    fn an_empty_file_declares_no_book() {
        let decls = parse("").unwrap();
        assert!(decls.books.is_empty());
        assert!(decls.presets.is_empty());
    }

    #[test]
    fn the_sample_of_init_reads_and_is_a_sound_declaration() {
        // `init::SAMPLE` is the public sample, the one `whyshelf init --sample`
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
