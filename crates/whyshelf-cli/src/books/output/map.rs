//! The answer of `map` ([`MapOut`]): what the declaration says, its books with their
//! keys and fields and its links with what they point at, and the drawing of them.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::MapReport;
use whyshelf_core::domain::shelf::artifact::Selection;
use whyshelf_core::domain::shelf::map::MapTarget;
use whyshelf_core::domain::shelf::schema::{
    ArtifactCover, CoverSeverity, FieldPresence, FieldType, KeyPattern, PresetOrigin,
    WithoutFrontMatter,
};

use super::words::schema_problem;

/// The answer of `map`: what the declaration says.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MapOut {
    /// The problems of the declaration, as `check` gives them; when there are any,
    /// there is no book, no link and no drawing.
    pub schema_problems: Vec<String>,
    /// The books, in the order of the declaration.
    pub books: Vec<MapBookOut>,
    /// The links, book by book and within a book in the order of the declaration.
    pub links: Vec<MapLinkOut>,
    /// The books and links drawn as text, one block per book; null when the
    /// declaration cannot be used.
    pub diagram: Option<String>,
}

/// A book of `map`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MapBookOut {
    /// The name.
    pub name: String,
    /// The directory.
    pub dir: String,
    /// The patterns of `include`, relative to the directory, as written; null when
    /// not written, which is every `.md` file under the directory.
    pub include: Option<Vec<String>>,
    /// The patterns of `exclude`, relative to the directory, as written; null when
    /// not written.
    pub exclude: Option<Vec<String>>,
    /// What a file in range with no front matter is, `invalid` or `skip`, as
    /// written; null when not written, which is `invalid`.
    pub without_front_matter: Option<&'static str>,
    /// The grammar of its keys.
    pub key: KeyGrammarOut,
    /// What it is for, when the declaration says.
    pub description: Option<String>,
    /// Its fields, in the order of the declaration.
    pub fields: Vec<MapFieldOut>,
}

/// The grammar of the keys of a book.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyGrammarOut {
    /// `<prefix>-<number or name>`.
    Prefix {
        /// The prefix, without the `-`.
        prefix: String,
    },
    /// A path of lowercase words.
    Slug,
}

/// A field of a book of `map`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MapFieldOut {
    /// The name.
    pub name: String,
    /// The type as the declaration writes it: `text`, `line`, `integer`, `bool`,
    /// `list` or `enum`.
    #[serde(rename = "type")]
    pub ty: &'static str,
    /// The words of an `enum`, sorted, not in the order written: the model keeps
    /// them as a set, as a pin's canon does. Empty for any other type.
    pub words: Vec<String>,
    /// Whether a document must carry it.
    pub required: bool,
}

/// A link of `map`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MapLinkOut {
    /// The book that holds it.
    pub from: String,
    /// The name.
    pub name: String,
    /// What it points at.
    pub target: MapTargetOut,
    /// `depends`, `informs` or `replaces`.
    pub means: &'static str,
    /// How many a document writes, as the declaration writes it: `one`,
    /// `optional`, `many` or `at-least-one`.
    pub count: &'static str,
    /// Whether the link is expected to reach what it covers, of either strength:
    /// every document of the target book that no decision went against (not denied,
    /// deprecated or retired), or, for a link to artifacts, every name its
    /// `selection` takes.
    pub cover: bool,
    /// How strongly it is expected, beside `cover`: `warn` or `error`; null when
    /// `cover` is false.
    pub cover_severity: Option<&'static str>,
    /// For a link to artifacts with a `cover`, the selection of the names of its
    /// space that it expects to reach, as the declaration writes it; null for any
    /// other link.
    pub selection: Option<SelectionOut>,
    /// For a link to artifacts whose `cover` declares `parts = { whole = "<link>" }`,
    /// that link of the book, through which each part of a listing is taken whole;
    /// null for any other link.
    pub parts_whole: Option<String>,
    /// The name of the preset a link to artifacts was expanded from, the one it
    /// names or the default; null when there is none, and for a link into a book.
    /// The values in `target` are the link's, whichever it is.
    pub preset: Option<String>,
    /// Whether `preset` is the default (`[defaults] artifact`), beside `preset`:
    /// false when the link names its preset, or has none.
    pub preset_by_default: bool,
}

/// The selection of the names a link to artifacts expects to reach, as the
/// declaration writes it: the texts are of the grammar of the space (for `file`, a
/// glob over the paths), and `map` keeps them as written.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SelectionOut {
    /// What takes names.
    pub include: Vec<String>,
    /// What leaves names out.
    pub exclude: Vec<String>,
}

impl SelectionOut {
    /// The selection `selection` is, as written.
    pub(super) fn of(selection: &Selection) -> Self {
        Self {
            include: selection.include().to_vec(),
            exclude: selection.exclude().to_vec(),
        }
    }
}

/// What a link of `map` points at.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapTargetOut {
    /// The documents of a book (which may be the book that holds the link).
    Book {
        /// The book.
        book: String,
    },
    /// Artifacts, by name in a space.
    Artifact {
        /// The space.
        space: String,
        /// What of each is compared: `bytes`, `lines` or `presence`.
        compare: &'static str,
        /// The prefixes of the lines `lines` leaves out, sorted, not in the order
        /// written: the model keeps them as a set, as a pin's canon does.
        ignore: Vec<String>,
    },
}

/// The answer of `map`.
pub fn map(report: &MapReport) -> MapOut {
    let map = match report {
        MapReport::SchemaInvalid(problems) => {
            return MapOut {
                schema_problems: problems.iter().map(schema_problem).collect(),
                books: Vec::new(),
                links: Vec::new(),
                diagram: None,
            };
        }
        MapReport::Mapped(map) => map,
    };
    MapOut {
        schema_problems: Vec::new(),
        books: map
            .books
            .iter()
            .map(|book| MapBookOut {
                name: book.name.to_string(),
                dir: book.dir.as_rel().to_string(),
                include: book
                    .scope
                    .include()
                    .map(|patterns| patterns.iter().map(ToString::to_string).collect()),
                exclude: book
                    .scope
                    .exclude()
                    .map(|patterns| patterns.iter().map(ToString::to_string).collect()),
                without_front_matter: book
                    .scope
                    .without_front_matter_declared()
                    .map(WithoutFrontMatter::word),
                key: match &book.key {
                    KeyPattern::Prefixed(prefix) => KeyGrammarOut::Prefix {
                        prefix: prefix.to_string(),
                    },
                    KeyPattern::Slug => KeyGrammarOut::Slug,
                },
                description: book.description.as_ref().map(ToString::to_string),
                fields: book
                    .fields
                    .iter()
                    .map(|(name, decl)| MapFieldOut {
                        name: name.to_string(),
                        ty: decl.ty.word(),
                        words: match &decl.ty {
                            FieldType::Enum(words) => {
                                words.iter().map(ToString::to_string).collect()
                            }
                            _ => Vec::new(),
                        },
                        required: decl.presence == FieldPresence::Required,
                    })
                    .collect(),
            })
            .collect(),
        links: map
            .links
            .iter()
            .map(|link| {
                let (cover, preset, selection, parts_whole) = match &link.target {
                    MapTarget::Book { cover, .. } => (*cover, None, None, None),
                    MapTarget::Artifact { preset, cover, .. } => (
                        cover.as_ref().map(ArtifactCover::severity),
                        preset.as_ref(),
                        cover.as_ref().map(|c| SelectionOut::of(c.selection())),
                        cover
                            .as_ref()
                            .and_then(ArtifactCover::parts_whole)
                            .map(ToString::to_string),
                    ),
                };
                MapLinkOut {
                    from: link.from.to_string(),
                    name: link.name.to_string(),
                    target: match &link.target {
                        MapTarget::Book { book, .. } => MapTargetOut::Book {
                            book: book.to_string(),
                        },
                        MapTarget::Artifact {
                            space, comparison, ..
                        } => MapTargetOut::Artifact {
                            space: space.to_string(),
                            compare: comparison.kind_word(),
                            ignore: comparison
                                .ignored()
                                .map(|p| p.as_str().to_owned())
                                .collect(),
                        },
                    },
                    means: link.propagation.word(),
                    count: link.cardinality.word(),
                    cover: cover.is_some(),
                    cover_severity: cover.map(CoverSeverity::word),
                    selection,
                    parts_whole,
                    preset: preset.map(|p| p.name().to_string()),
                    preset_by_default: preset.is_some_and(PresetOrigin::is_default),
                }
            })
            .collect(),
        diagram: Some(super::diagram::draw(map)),
    }
}
