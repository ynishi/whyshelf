//! The check of a declaration: what is read of each book ([`Draft`]), and the checks
//! that span the books, each collecting its problems with the others, so that
//! [`Schema::validate`](super::Schema::validate) returns every problem at once.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::nonempty::{Count, NonEmptySet};

use super::super::artifact::{Comparison, Space};
use super::{
    ARTIFACT_TARGET, ArtifactLinkKind, ArtifactPropagation, BookDescription, BookDir, BookName,
    BookRange, BookSchema, BookSchemaDecl, BookScope, Cardinality, CoverSeverity, Declared,
    DeclaredLink, EntryName, FieldDecl, FieldEntryDecl, FieldType, FieldTypeDecl, FieldWord,
    KeyPattern, KeyPatternDecl, KeyPrefix, KeyWord, LinkEntryDecl, LinkKind, LinkTargetDecl,
    NamePlace, Origins, PresetOrigin, Propagation, SchemaProblem, WordAt, offered, read_book_word,
    read_scope, read_word, report,
};

/// What [`Schema::validate`](super::Schema::validate) learns of one declaration before it can be judged
/// against the others. Every part that could be read is kept, so the problems that
/// span books are reported even when a book has a problem of its own.
pub(super) struct Draft {
    label: String,
    pub(super) name: Option<BookName>,
    dir: Option<BookDir>,
    scope: Option<BookScope>,
    pub(super) key: Option<KeyPattern>,
    description: Option<BookDescription>,
    fields: Declared<EntryName, FieldDecl>,
    links: Declared<EntryName, DraftEntry>,
    presets: BTreeMap<EntryName, PresetOrigin>,
    clean: bool,
}

/// A link of a draft, into a book or to artifacts.
enum DraftEntry {
    Book(DraftLink),
    Artifact(ArtifactLinkKind),
}

/// A link of a draft into a book: what is read of the declaration, before the
/// books are known together (the grammar of the target's keys is added when they
/// are).
struct DraftLink {
    target: BookName,
    cardinality: Cardinality,
    propagation: Propagation,
    cover: Option<CoverSeverity>,
}

impl Draft {
    pub(super) fn read(
        decl: BookSchemaDecl,
        spaces: &BTreeSet<Space>,
        origins: &Origins,
        problems: &mut Vec<SchemaProblem>,
    ) -> Self {
        let label = decl.name.clone();
        let first_problem = problems.len();
        let name = read_book_name(&decl.name, problems);
        let dir = read_dir(&label, &decl.dir, problems);
        let key = read_key_pattern(&label, &decl.key, problems);
        let scope = read_scope(&label, decl.scope, problems);
        let description = read_description(&label, decl.description.as_deref(), problems);
        let fields = read_fields(&label, decl.fields, problems);
        let links = read_links(&label, decl.links, &fields, spaces, problems);
        let presets = links
            .iter()
            .filter(|(_, entry)| matches!(entry, DraftEntry::Artifact(_)))
            .filter_map(|(link, _)| Some((link.clone(), origins.of(&label, link.as_str())?)))
            .collect();
        Self {
            label,
            name,
            dir,
            scope,
            key,
            description,
            fields,
            links,
            presets,
            clean: problems.len() == first_problem,
        }
    }

    /// The links of the draft into books.
    fn book_links(&self) -> impl Iterator<Item = (&EntryName, &DraftLink)> {
        self.links.iter().filter_map(|(name, entry)| match entry {
            DraftEntry::Book(link) => Some((name, link)),
            DraftEntry::Artifact(_) => None,
        })
    }

    /// The book, when the declaration had no problem of its own. `key_of` gives the
    /// grammar of the keys of every book whose name and key were read; a link into
    /// a book that is not there was refused before, so a clean draft finds its
    /// targets.
    pub(super) fn into_schema(self, key_of: &BTreeMap<BookName, KeyPattern>) -> Option<BookSchema> {
        if !self.clean {
            return None;
        }
        let mut links = Declared::default();
        for (name, entry) in self.links.into_entries() {
            let link = match entry {
                DraftEntry::Artifact(kind) => DeclaredLink::Artifact(kind),
                DraftEntry::Book(link) => DeclaredLink::Book(LinkKind {
                    target_key: key_of.get(&link.target)?.clone(),
                    target: link.target,
                    cardinality: link.cardinality,
                    propagation: link.propagation,
                    cover: link.cover,
                }),
            };
            links.insert(name, link);
        }
        Some(BookSchema {
            name: self.name?,
            range: BookRange::new(self.dir?, self.scope?),
            key: self.key?,
            description: self.description,
            fields: self.fields,
            links,
            presets: self.presets,
        })
    }
}

fn read_description(
    book: &str,
    text: Option<&str>,
    problems: &mut Vec<SchemaProblem>,
) -> Option<BookDescription> {
    match BookDescription::new(text?) {
        Ok(description) => Some(description),
        Err(why) => {
            problems.push(SchemaProblem::BadDescription {
                book: book.to_owned(),
                why,
            });
            None
        }
    }
}

fn read_book_name(text: &str, problems: &mut Vec<SchemaProblem>) -> Option<BookName> {
    if text == ARTIFACT_TARGET {
        problems.push(SchemaProblem::ReservedBookName);
        return None;
    }
    match BookName::new(text) {
        Ok(name) => Some(name),
        Err(why) => {
            problems.push(SchemaProblem::BadName {
                place: NamePlace::Book,
                text: text.to_owned(),
                why,
            });
            None
        }
    }
}

fn read_dir(book: &str, text: &str, problems: &mut Vec<SchemaProblem>) -> Option<BookDir> {
    match BookDir::new(text) {
        Ok(dir) => Some(dir),
        Err(source) => {
            problems.push(SchemaProblem::BadDir {
                book: book.to_owned(),
                source,
            });
            None
        }
    }
}

pub(super) fn read_key_pattern(
    book: &str,
    decl: &KeyPatternDecl,
    problems: &mut Vec<SchemaProblem>,
) -> Option<KeyPattern> {
    match decl {
        KeyPatternDecl::Word(word) => {
            report(read_book_word(word, book), problems).map(|word| match word {
                KeyWord::Slug => KeyPattern::Slug,
            })
        }
        KeyPatternDecl::Prefixed { prefix } => match KeyPrefix::new(prefix) {
            Ok(prefix) => Some(KeyPattern::Prefixed(prefix)),
            Err(why) => {
                problems.push(SchemaProblem::BadKeyPrefix {
                    book: book.to_owned(),
                    why,
                });
                None
            }
        },
    }
}

/// The fields of a book, in the order written; one that breaks a rule is reported
/// and left out.
fn read_fields(
    book: &str,
    entries: Vec<FieldEntryDecl>,
    problems: &mut Vec<SchemaProblem>,
) -> Declared<EntryName, FieldDecl> {
    let mut fields = Declared::default();
    for entry in entries {
        let name = match EntryName::new(&entry.name) {
            Ok(name) => name,
            Err(why) => {
                problems.push(SchemaProblem::BadName {
                    place: NamePlace::Field {
                        book: book.to_owned(),
                    },
                    text: entry.name,
                    why,
                });
                continue;
            }
        };
        let Some(ty) = read_field_type(book, &entry.name, entry.ty, problems) else {
            continue;
        };
        let decl = FieldDecl {
            ty,
            presence: entry.presence,
        };
        if !fields.insert(name.clone(), decl) {
            problems.push(SchemaProblem::DuplicateField {
                book: book.to_owned(),
                name,
            });
        }
    }
    fields
}

/// The type of a field; none when its word is no type, or it is an enum that cannot
/// be made (the problem is reported).
fn read_field_type(
    book: &str,
    field: &str,
    ty: FieldTypeDecl,
    problems: &mut Vec<SchemaProblem>,
) -> Option<FieldType> {
    let words = match ty {
        FieldTypeDecl::Word(word) => {
            let at = WordAt::Field {
                book: book.to_owned(),
                field: field.to_owned(),
            };
            return report(read_word(&word, at), problems).map(FieldType::Scalar);
        }
        FieldTypeDecl::Enum(words) => words,
    };
    let mut set = BTreeSet::new();
    let before_words = problems.len();
    for word in words {
        match FieldWord::new(&word) {
            Ok(word) => {
                set.insert(word);
            }
            Err(why) => problems.push(SchemaProblem::BadName {
                place: NamePlace::EnumWord {
                    book: book.to_owned(),
                    field: field.to_owned(),
                },
                text: word,
                why,
            }),
        }
    }
    match NonEmptySet::from_set(set) {
        Ok(set) => Some(FieldType::Enum(set)),
        Err(_) => {
            // A word that broke the grammar was reported above; an enum that listed
            // nothing at all is the problem here.
            if problems.len() == before_words {
                problems.push(SchemaProblem::EmptyEnum {
                    book: book.to_owned(),
                    field: field.to_owned(),
                });
            }
            None
        }
    }
}

/// The links of a book, into books and to artifacts; one that breaks a rule is
/// reported and left out. `fields` are the fields already read, for a name that is
/// both. A name is one link, whichever its target: a name written twice is reported
/// whether the two point at books or at artifacts. `spaces` are the spaces the
/// adapters offer. The links kept are in the order written.
fn read_links(
    book: &str,
    entries: Vec<LinkEntryDecl>,
    fields: &Declared<EntryName, FieldDecl>,
    spaces: &BTreeSet<Space>,
    problems: &mut Vec<SchemaProblem>,
) -> Declared<EntryName, DraftEntry> {
    let mut links = Declared::default();
    for entry in entries {
        let name = match EntryName::new(&entry.name) {
            Ok(name) => name,
            Err(why) => {
                problems.push(SchemaProblem::BadName {
                    place: NamePlace::Link {
                        book: book.to_owned(),
                    },
                    text: entry.name,
                    why,
                });
                continue;
            }
        };
        let target = match entry.target {
            LinkTargetDecl::Artifact => None,
            LinkTargetDecl::Book(target) => match BookName::new(&target) {
                Ok(target) => Some(target),
                Err(why) => {
                    problems.push(SchemaProblem::BadName {
                        place: NamePlace::LinkTarget {
                            book: book.to_owned(),
                            link: entry.name,
                        },
                        text: target,
                        why,
                    });
                    continue;
                }
            },
        };
        if fields.contains_key(&name) {
            problems.push(SchemaProblem::NameCollision {
                book: book.to_owned(),
                name: name.clone(),
            });
        }
        if links.contains_key(&name) {
            problems.push(SchemaProblem::DuplicateLink {
                book: book.to_owned(),
                name: name.clone(),
            });
        }
        let cover = match entry.cover.as_ref().map(CoverSeverity::read) {
            None => None,
            Some(Ok(cover)) => cover,
            Some(Err(value)) => {
                problems.push(SchemaProblem::BadCover {
                    book: book.to_owned(),
                    link: name,
                    value,
                });
                continue;
            }
        };
        match target {
            Some(target) => {
                if entry.compare.is_some() {
                    problems.push(SchemaProblem::CompareOnBookLink {
                        book: book.to_owned(),
                        link: name,
                    });
                    continue;
                }
                if entry.space.is_some() {
                    problems.push(SchemaProblem::SpaceOnBookLink {
                        book: book.to_owned(),
                        link: name,
                    });
                    continue;
                }
                let link = DraftLink {
                    target,
                    cardinality: entry.cardinality,
                    propagation: entry.propagation,
                    cover,
                };
                links.insert(name, DraftEntry::Book(link));
            }
            None => {
                if cover.is_some() {
                    problems.push(SchemaProblem::CoverOnArtifactLink {
                        book: book.to_owned(),
                        link: name,
                    });
                    continue;
                }
                let Some(space) = read_space(book, &name, entry.space.as_deref(), spaces, problems)
                else {
                    continue;
                };
                let comparison = match &entry.compare {
                    None => Comparison::Bytes,
                    Some(decl) => match Comparison::parse(&decl.kind, &decl.ignore) {
                        Ok(comparison) => comparison,
                        Err(why) => {
                            problems.push(SchemaProblem::BadComparison {
                                book: book.to_owned(),
                                link: name,
                                why,
                            });
                            continue;
                        }
                    },
                };
                let propagation = match entry.propagation {
                    Propagation::Depends => ArtifactPropagation::Depends,
                    Propagation::Informs => ArtifactPropagation::Informs,
                    Propagation::Replaces => {
                        problems.push(SchemaProblem::ArtifactReplaces {
                            book: book.to_owned(),
                            link: name,
                        });
                        continue;
                    }
                };
                let link = ArtifactLinkKind {
                    space,
                    cardinality: entry.cardinality,
                    propagation,
                    comparison,
                };
                links.insert(name, DraftEntry::Artifact(link));
            }
        }
    }
    links
}

/// The space of a link to artifacts: written, a space, and one the adapters offer
/// (the spaces come from the port, as a value). Each of the three that is not so is
/// reported.
fn read_space(
    book: &str,
    link: &EntryName,
    written: Option<&str>,
    spaces: &BTreeSet<Space>,
    problems: &mut Vec<SchemaProblem>,
) -> Option<Space> {
    let Some(written) = written else {
        problems.push(SchemaProblem::NoSpace {
            book: book.to_owned(),
            link: link.clone(),
        });
        return None;
    };
    let space = match Space::new(written) {
        Ok(space) => space,
        Err(why) => {
            problems.push(SchemaProblem::BadSpace {
                book: book.to_owned(),
                link: link.clone(),
                why,
            });
            return None;
        }
    };
    if !spaces.contains(&space) {
        problems.push(SchemaProblem::UnknownSpace {
            book: book.to_owned(),
            link: link.clone(),
            space,
            offered: offered(spaces),
        });
        return None;
    }
    Some(space)
}

/// The names of the books declared, each once; a name written twice is reported
/// (once, however many times it is written).
pub(super) fn declared_names<'a>(
    drafts: &'a [Draft],
    problems: &mut Vec<SchemaProblem>,
) -> BTreeSet<&'a BookName> {
    let mut seen = BTreeSet::new();
    let mut reported = BTreeSet::new();
    for name in drafts.iter().filter_map(|d| d.name.as_ref()) {
        if !seen.insert(name) && reported.insert(name) {
            problems.push(SchemaProblem::DuplicateBook { name: name.clone() });
        }
    }
    seen
}

/// A link into a book that is not declared, and a `Replaces` link that leaves its
/// own book.
pub(super) fn check_links(
    drafts: &[Draft],
    declared: &BTreeSet<&BookName>,
    problems: &mut Vec<SchemaProblem>,
) {
    for draft in drafts {
        for (link, kind) in draft.book_links() {
            if !declared.contains(&kind.target) {
                problems.push(SchemaProblem::UnknownLinkTarget {
                    book: draft.label.clone(),
                    link: link.clone(),
                    target: kind.target.clone(),
                });
            }
            if kind.propagation == Propagation::Replaces
                && draft.name.as_ref().is_some_and(|own| *own != kind.target)
            {
                problems.push(SchemaProblem::ReplacesForeign {
                    book: draft.label.clone(),
                    link: link.clone(),
                    target: kind.target.clone(),
                });
            }
        }
    }
}

/// Two or more books that use one key prefix.
pub(super) fn check_prefixes(drafts: &[Draft], problems: &mut Vec<SchemaProblem>) {
    let mut by_prefix: BTreeMap<&KeyPrefix, BTreeSet<&BookName>> = BTreeMap::new();
    for draft in drafts {
        if let (Some(name), Some(KeyPattern::Prefixed(prefix))) = (&draft.name, &draft.key) {
            by_prefix.entry(prefix).or_default().insert(name);
        }
    }
    for (prefix, books) in by_prefix {
        if let Count::Many(books) = Count::of(books.into_iter().cloned()) {
            problems.push(SchemaProblem::PrefixClash {
                prefix: prefix.clone(),
                books,
            });
        }
    }
}

/// Two books whose directories are the same or nest.
pub(super) fn check_dirs(drafts: &[Draft], problems: &mut Vec<SchemaProblem>) {
    for (i, a) in drafts.iter().enumerate() {
        for b in &drafts[i + 1..] {
            if let (Some(an), Some(ad), Some(bn), Some(bd)) = (&a.name, &a.dir, &b.name, &b.dir)
                && an != bn
                && ad.overlaps(bd)
            {
                problems.push(SchemaProblem::DirOverlap {
                    first: an.clone(),
                    second: bn.clone(),
                });
            }
        }
    }
}
