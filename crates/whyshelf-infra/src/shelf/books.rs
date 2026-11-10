//! The declaration of the books: `.whyshelf/books.toml`.
//!
//! ```toml
//! [[book]]
//! name = "finding"
//! dir = "docs/findings"
//! key = { prefix = "F" }          # keys F-1, F-2, F-name; or key = "slug"
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
//!
//! [[book.link]]
//! name = "tests"
//! target = "artifact"              # files that are no document, by path under the root
//! means = "depends"                # "depends" or "informs" (an artifact replaces nothing)
//! ```
//!
//! The word `artifact` as a target is read here into
//! [`LinkTargetDecl::Artifact`]; the domain refuses a book of that name, so the word
//! never names both.
//!
//! This module reads the text and its shape; whether the books make sense (names,
//! directories that overlap, links to a book that is not declared) is
//! [`Schema::validate`](whyshelf_core::domain::shelf::schema::Schema::validate)'s
//! work. Fields and links stay lists, so a name written twice reaches the domain
//! twice and is reported there.

use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use whyshelf_core::app::shelf::BookSchemaSource;
use whyshelf_core::domain::shelf::schema::{
    ARTIFACT_TARGET, BookSchemaDecl, Cardinality, FieldEntryDecl, FieldPresence, FieldTypeDecl,
    KeyPatternDecl, LinkEntryDecl, LinkTargetDecl, Propagation,
};

/// A declaration of a Finding -> Spec -> Design chain, as a project could write it.
///
/// whyshelf has no kind of document of its own; this sample is data, not a mode of
/// the program. Its test checks that it reads and that
/// [`Schema::validate`](whyshelf_core::domain::shelf::schema::Schema::validate)
/// accepts it, so the manual that shows it cannot drift from what the program reads.
///
#[doc = concat!("```toml\n", include_str!("books.sample.toml"), "```")]
pub const SAMPLE: &str = include_str!("books.sample.toml");

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
        /// The file.
        path: PathBuf,
        /// What the parser said.
        #[source]
        source: toml::de::Error,
    },
}

/// The declaration of the books, read from a TOML file.
#[derive(Debug, Clone)]
pub struct BooksToml {
    path: PathBuf,
}

impl BooksToml {
    /// The declaration in the file at `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl BookSchemaSource for BooksToml {
    type Error = BooksError;

    async fn declarations(&self) -> Result<Vec<BookSchemaDecl>, BooksError> {
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
        parse(&text).map_err(|source| BooksError::Shape {
            path: self.path.clone(),
            source,
        })
    }
}

/// The declarations a TOML text holds.
pub fn parse(text: &str) -> Result<Vec<BookSchemaDecl>, toml::de::Error> {
    let file: FileDto = toml::from_str(text)?;
    Ok(file.book.into_iter().map(BookDto::into_decl).collect())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDto {
    #[serde(default)]
    book: Vec<BookDto>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BookDto {
    name: String,
    dir: String,
    key: KeyDto,
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
    means: MeansWord,
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

impl BookDto {
    fn into_decl(self) -> BookSchemaDecl {
        BookSchemaDecl {
            name: self.name,
            dir: self.dir,
            key: match self.key {
                KeyDto::Prefixed(PrefixDto { prefix }) => KeyPatternDecl::Prefixed { prefix },
                KeyDto::Word(KeyWord::Slug) => KeyPatternDecl::Slug,
            },
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
                .map(|l| LinkEntryDecl {
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
                    propagation: match l.means {
                        MeansWord::Depends => Propagation::Depends,
                        MeansWord::Informs => Propagation::Informs,
                        MeansWord::Replaces => Propagation::Replaces,
                    },
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].name, "finding");
        assert_eq!(
            decls[0].key,
            KeyPatternDecl::Prefixed {
                prefix: "F".to_owned()
            }
        );
        assert_eq!(
            decls[0].fields[0].ty,
            FieldTypeDecl::Enum(vec!["low".to_owned(), "high".to_owned()])
        );
        assert_eq!(decls[0].fields[0].presence, FieldPresence::Required);
        assert_eq!(decls[1].key, KeyPatternDecl::Slug);
        assert_eq!(decls[1].links[0].cardinality, Cardinality::AtLeastOne);
        assert_eq!(decls[1].links[0].propagation, Propagation::Depends);
        assert_eq!(decls[1].links[1].cardinality, Cardinality::Many);
        assert_eq!(decls[1].links[1].propagation, Propagation::Informs);
    }

    #[test]
    fn the_target_artifact_is_a_link_to_files_and_not_to_a_book() {
        let decls = parse(
            "[[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [[book.link]]\nname='tests'\ntarget='artifact'\nmeans='depends'\n",
        )
        .unwrap();
        assert_eq!(decls[0].links[0].target, LinkTargetDecl::Artifact);
        let schema = whyshelf_core::domain::shelf::schema::Schema::validate(decls).unwrap();
        assert_eq!(schema.book("a").unwrap().artifact_links().len(), 1);
    }

    #[test]
    fn a_key_the_shape_does_not_know_is_refused() {
        assert!(parse("[[book]]\nname='a'\ndir='a'\nkey='slug'\ncolour='red'\n").is_err());
        assert!(parse("[[book]]\nname='a'\ndir='a'\nkey='uuid'\n").is_err());
    }

    #[test]
    fn an_empty_file_declares_no_book() {
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn the_sample_reads_and_is_a_sound_declaration() {
        // `super::SAMPLE` is the public sample; this module has a small one of its own.
        let decls = parse(super::SAMPLE).unwrap();
        let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["finding", "spec", "design"]);
        let schema = whyshelf_core::domain::shelf::schema::Schema::validate(decls).unwrap();
        assert_eq!(schema.books().len(), 3);
        let design = schema.book("design").unwrap();
        assert!(design.artifact_links().contains_key("tests"));
    }

    #[tokio::test]
    async fn a_missing_file_is_an_error_and_not_an_empty_shelf() {
        let dir = tempfile::tempdir().unwrap();
        let source = BooksToml::new(dir.path().join("books.toml"));
        assert!(matches!(
            source.declarations().await,
            Err(BooksError::Missing { .. })
        ));
    }
}
