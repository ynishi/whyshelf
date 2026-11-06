//! The public boundary: what text leaves a person's private side, and whether it
//! may.
//!
//! A person has private documents and a private ledger. Some text is written where
//! everyone who clones the project reads it: a judgement copied into the public
//! ledger, the full text of an object kept beside it, the body and front matter of
//! a document that is made public, the path it is put at. This module lists that
//! text, from the fields of the types that cross, and checks it for what must not
//! cross: the alias or the title of a private document, and the id of one written
//! without its `private:` mark (the rule of [`leak_lint`], which is used and not
//! restated here).
//!
//! # The text that crosses
//!
//! [`CrossingText::of`] gives, for a value of a type that crosses, every piece of
//! prose it carries, each labelled with its [`CrossingField`]. It is written for
//! each type as an exhaustive destructuring or match over its fields (beside the
//! type where the fields are private, here where they are public), so adding a field
//! is a compile error until its text is listed or said to carry none. The types that
//! cross:
//!
//! | type | what is listed |
//! |---|---|
//! | [`Judgement`](super::judgement::Judgement) and its [kinds](super::judgement::JudgementKind) | who decided (name, mail, the tool of an agent), the reason of a revocation, a retirement or a purge, the source of an adoption, the hint of a measurement, the revision of the code, the name of the key of each keyed pin |
//! | [`ObjectText`] | the full text of a version kept for an approval |
//! | [`DocumentText`] | the front matter and the body of a document made public |
//! | [`SourceUri`](super::location::SourceUri), [`Judge`](super::judge::Judge), [`RelPath`] | the text of each |
//!
//! Identifiers and hashes are not listed: they are not prose, and a ledger is
//! meant to hold them. (The name of a key is listed: a person chose it.)
//!
//! A path is checked as prose is, and also by the names it is built from, so that a
//! destination such as `design/x.md` is a leak where `design/x` is a private alias
//! ([`PrivateNames::leaks_in`]). (A pin of a private upstream is keyed so that its hash is
//! not a fingerprint: see [`keyring`](super::keyring).)
//!
//! Whether the pieces hold a secret is for an adapter to say, with a pattern
//! matcher; this module lists the texts and does not scan for secrets.
//!
//! # Who can be checked: [`PrivateRoot`]
//!
//! The check needs the private documents, and it covers only this person's: the
//! private documents of someone else are not visible here. What the check can say
//! depends on the person's private root, a fact of their own settings:
//!
//! | [`PrivateRoot`] | the check |
//! |---|---|
//! | `NotUsedHere` | passes: this person has no private documents here (CI, a colleague without a private root) |
//! | `Present` | runs against the private documents of the corpus |
//! | `Absent` | unknown: a root is set and cannot be read. Writing to the public ledger is refused ([`LeakRefusal::Uncheckable`]) |
//!
//! The commands that show the settings state the value and where it came from
//! (otherwise a settings file that is not read where it should be, in another
//! container or shell, would turn `Present` into `NotUsedHere` and let a leak through
//! unseen).

use std::collections::BTreeSet;

use super::approval::{Attestation, CmResult};
use super::corpus::{Corpus, Entity};
use super::error::LeakRefusal;
use super::hash::TextHash;
use super::id::{DocId, EntityId};
use super::knowable::Knowable;
use super::lint::{LeakFinding, PrivateName, leak_lint};
use super::location::RelPath;
use super::nonempty::NonEmptyVec;
use super::revision::Revision;

/// Whether this person has a private root here, and whether it can be read.
///
/// Decided by the person's own settings only, never by the committed ones: a setting
/// that anyone can edit must not be able to switch the check off. The application
/// reads the settings and the disk and passes the value in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrivateRoot {
    /// The person's settings name no private root (or say `none`): they have no
    /// private documents here.
    NotUsedHere,
    /// The settings name a root and it can be read.
    Present,
    /// The settings name a root and it cannot be read (a missing directory, no
    /// permission, a disk that is not attached), or the settings file is there and
    /// cannot be read.
    Absent,
}

/// The field a piece of crossing text comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CrossingField {
    /// The name of a judge.
    JudgeName,
    /// The mail of a judge.
    JudgeEmail,
    /// The tool an agent declared.
    JudgeTool,
    /// Why a revocation, a retirement or a purge was made.
    Reason,
    /// Where an adopted decision came from.
    Source,
    /// The hint at a missing item in a measurement.
    ItemHint,
    /// The revision of the code an approval checked.
    Revision,
    /// The full text of a kept version.
    Object,
    /// The front matter of a document.
    FrontMatter,
    /// The body of a document.
    Body,
    /// The path a document is put at.
    Path,
    /// The name of the key a pin of a private upstream was made under.
    KeyName,
}

impl CrossingField {
    /// Whether the text of the field is a path, whose segments and extension end a
    /// name where prose would not ([`PrivateNames::leaks_in`]).
    fn is_path_like(self) -> bool {
        match self {
            Self::Path => true,
            Self::JudgeName
            | Self::JudgeEmail
            | Self::JudgeTool
            | Self::Reason
            | Self::Source
            | Self::ItemHint
            | Self::Revision
            | Self::Object
            | Self::FrontMatter
            | Self::Body
            | Self::KeyName => false,
        }
    }
}

/// One piece of text that crosses, and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    field: CrossingField,
    text: String,
}

impl Piece {
    /// The field it comes from.
    pub fn field(&self) -> CrossingField {
        self.field
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// The text of a value that is written where others read it.
///
/// Built by [`of`](Self::of) from a type that implements [`Crosses`]. The list is
/// what the secret scan of an adapter and the leak check read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrossingText {
    pieces: Vec<Piece>,
}

impl CrossingText {
    /// The text that crosses with `value`.
    pub fn of<T: Crosses + ?Sized>(value: &T) -> Self {
        let mut text = Self::default();
        value.cross_into(&mut text);
        text
    }

    /// Add one piece. A blank text is not a piece.
    pub fn push(&mut self, field: CrossingField, text: impl Into<String>) {
        let text = text.into();
        if !text.trim().is_empty() {
            self.pieces.push(Piece { field, text });
        }
    }

    /// Add the pieces of another value.
    pub fn include<T: Crosses + ?Sized>(&mut self, value: &T) {
        value.cross_into(self);
    }

    /// The pieces, in the order they were added.
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// Whether nothing crosses.
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }
}

/// A type whose value can be written where others read it.
///
/// Implemented beside each type as an exhaustive match over its fields: see the
/// module documentation.
pub trait Crosses {
    /// Add the pieces of text of this value to `out`.
    fn cross_into(&self, out: &mut CrossingText);
}

impl Crosses for RelPath {
    fn cross_into(&self, out: &mut CrossingText) {
        out.push(CrossingField::Path, self.as_str());
    }
}

impl Crosses for Revision {
    fn cross_into(&self, out: &mut CrossingText) {
        match self {
            Self::Commit(id) => out.push(CrossingField::Revision, id.as_str()),
            Self::Patch { base, patch: _ } => out.push(CrossingField::Revision, base.as_str()),
        }
    }
}

impl Crosses for CmResult {
    fn cross_into(&self, out: &mut CrossingText) {
        match self {
            Self::Pass => {}
            Self::FailWiring { item_hint } => out.push(CrossingField::ItemHint, item_hint.as_str()),
            Self::FailHyp { finding: _ } => {}
        }
    }
}

impl Crosses for Attestation {
    fn cross_into(&self, out: &mut CrossingText) {
        match self {
            Self::Cr { finding: _ } => {}
            Self::Cs {
                spec: _,
                hyp: _,
                grounds,
                policy: _,
            } => {
                for ground in grounds.iter() {
                    ground.pin().cross_into(out);
                }
            }
            Self::Ci { design: _, spec } => spec.pin().cross_into(out),
            Self::Ca {
                item: _,
                design,
                revision,
                tests: _,
                change,
            } => {
                design.pin().cross_into(out);
                revision.cross_into(out);
                // The other cases are a fixed word: they say nothing a reader could use.
                if let Some(fingerprint) = change.fingerprint() {
                    fingerprint.cross_into(out);
                }
            }
            Self::Cm {
                spec: _,
                cs: _,
                result,
            } => result.cross_into(out),
        }
    }
}

/// The full text of a version kept for an approval: the object that is copied
/// beside the records.
///
/// Its key is made from the text, so the two cannot disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectText {
    key: TextHash,
    text: String,
}

impl ObjectText {
    /// An object of a text; the text is the normalised full text of the version.
    pub fn new(text: String) -> Self {
        Self {
            key: TextHash::of(&text),
            text,
        }
    }

    /// The hash the object is kept under.
    pub fn key(&self) -> TextHash {
        self.key
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Crosses for ObjectText {
    fn cross_into(&self, out: &mut CrossingText) {
        let Self { key: _, text } = self;
        out.push(CrossingField::Object, text.as_str());
    }
}

/// The text of a document file that is made public: what is between the `---`
/// lines, and what follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentText {
    front_matter: String,
    body: String,
}

impl DocumentText {
    /// The two parts of a file, as the adapter that read it split them.
    pub fn new(front_matter: String, body: String) -> Self {
        Self { front_matter, body }
    }

    /// The two parts of a file, read from its raw bytes.
    ///
    /// The bytes are decoded as UTF-8, and a byte that is not is read as `U+FFFD`;
    /// that is enough for a check, which looks for names, and no byte is left out of
    /// it. The front matter is what is between a first line that is `---` and the
    /// next such line; a file with no such pair is all body.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let text = String::from_utf8_lossy(bytes);
        let mut front_start = None;
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let end = offset + line.len();
            if line.trim_end() != "---" {
                // The first line is not a fence, or the fence is never closed.
                if front_start.is_none() {
                    break;
                }
            } else if let Some(start) = front_start {
                return Self::new(text[start..offset].to_owned(), text[end..].to_owned());
            } else {
                front_start = Some(end);
            }
            offset = end;
        }
        Self::new(String::new(), text.into_owned())
    }
}

impl Crosses for DocumentText {
    fn cross_into(&self, out: &mut CrossingText) {
        let Self { front_matter, body } = self;
        out.push(CrossingField::FrontMatter, front_matter.as_str());
        out.push(CrossingField::Body, body.as_str());
    }
}

/// The names and identifiers of this person's private documents: what must not
/// appear in text that crosses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrivateNames {
    names: Vec<PrivateName>,
    ids: Vec<EntityId>,
}

impl PrivateNames {
    /// The names of the private documents of the corpus, except those of `except`
    /// (a document that is being made public, whose own alias and title belong in
    /// its text), as far as `root` lets them be known.
    ///
    /// A document is private by [`Corpus::is_private`], so one whose publication
    /// is not recorded yet still counts. The ids listed are those of the documents
    /// and of the items of their Designs.
    pub fn of(corpus: &Corpus, root: PrivateRoot, except: &BTreeSet<DocId>) -> Knowable<Self> {
        match root {
            PrivateRoot::Absent => return Knowable::Unknown,
            PrivateRoot::NotUsedHere => return Knowable::Known(Self::default()),
            PrivateRoot::Present => {}
        }
        let mut known = Self::default();
        for file in corpus.documents() {
            let id = file.id();
            if except.contains(&id) || !corpus.is_private(&id) {
                continue;
            }
            let document = file.document();
            known.ids.push(id.entity());
            if let Some(alias) = document.header().alias() {
                known.names.push(PrivateName::Alias(alias.clone()));
            }
            match document.entity() {
                Entity::Finding(finding) => known
                    .names
                    .push(PrivateName::Title(finding.title().clone())),
                Entity::Spec(_) => {}
                Entity::Design(design) => known.ids.extend(
                    design
                        .items()
                        .values()
                        .filter_map(|item| item.id())
                        .map(|item| item.entity()),
                ),
            }
        }
        Knowable::Known(known)
    }

    /// Whether no private document is known.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.ids.is_empty()
    }

    /// Every leak in `text`, in the order of its pieces.
    ///
    /// A piece of prose is read by [`leak_lint`] as it is. A path is read so too, and
    /// also as the names it is built from: in `design/x.md` the alias `design/x` is
    /// followed by `.md`, which prose would read as more of the same token, but a
    /// path ends a name at a `/` and before an extension, so every run of whole
    /// segments, the last one with or without its extensions, is read as a name.
    pub fn leaks_in(&self, text: &CrossingText) -> Vec<Leak> {
        let mut leaks = Vec::new();
        for piece in text.pieces() {
            let mut found = leak_lint(piece.text(), &self.names, &self.ids);
            if piece.field().is_path_like() {
                for name in path_names(piece.text()) {
                    for finding in leak_lint(&name, &self.names, &self.ids) {
                        if !found.contains(&finding) {
                            found.push(finding);
                        }
                    }
                }
            }
            leaks.extend(found.into_iter().map(|finding| Leak {
                field: piece.field(),
                finding,
            }));
        }
        leaks
    }
}

/// The names a path is built from: every run of whole segments, with the last
/// segment of the run also cut before each of its dots (`a/b.tar.gz` gives `a`,
/// `a/b`, `a/b.tar`, `a/b.tar.gz`, `b`, `b.tar` and `b.tar.gz`).
///
/// A segment that starts with a dot is not cut there: `.config` is a name, not an
/// extension.
fn path_names(path: &str) -> BTreeSet<String> {
    let segments: Vec<&str> = path.split('/').collect();
    let mut names = BTreeSet::new();
    for start in 0..segments.len() {
        for end in start..segments.len() {
            let head = segments[start..end].join("/");
            let last = segments[end];
            let cuts = last
                .match_indices('.')
                .map(|(at, _)| at)
                .filter(|at| *at > 0)
                .chain([last.len()]);
            for cut in cuts {
                let name = if head.is_empty() {
                    last[..cut].to_owned()
                } else {
                    format!("{head}/{}", &last[..cut])
                };
                names.insert(name);
            }
        }
    }
    names
}

/// A private name, or an id without its mark, found in text that crosses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
    field: CrossingField,
    finding: LeakFinding,
}

impl Leak {
    /// The field the text came from.
    pub fn field(&self) -> CrossingField {
        self.field
    }

    /// What was found, and on which line of the field.
    pub fn finding(&self) -> &LeakFinding {
        &self.finding
    }
}

/// Check text that is about to be written where others read it.
///
/// `names` is what [`PrivateNames::of`] says: when it is unknown the answer is
/// [`LeakRefusal::Uncheckable`], and writing is refused with it; when there is a leak
/// the answer lists all of them.
pub fn check_crossing(
    text: &CrossingText,
    names: &Knowable<PrivateNames>,
) -> Result<(), LeakRefusal> {
    let Knowable::Known(names) = names else {
        return Err(LeakRefusal::Uncheckable);
    };
    match NonEmptyVec::from_vec(names.leaks_in(text)) {
        Ok(leaks) => Err(LeakRefusal::Leaks(leaks)),
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::adoption::{self, AdoptionInput};
    use crate::domain::approval::{Gate, Pinned};
    use crate::domain::corpus::{
        Document, FileFacts, GitPlace, Header, Location, Reading, Visibility,
    };
    use crate::domain::design::test_support::{design, identified};
    use crate::domain::finding::test_support::finding;
    use crate::domain::gate::Passed;
    use crate::domain::hash::BytesHash;
    use crate::domain::id::test_support::{docid, fid, iid, sid};
    use crate::domain::judge::{Email, Judge, PersonName, Principal, ToolName};
    use crate::domain::judgement::ObjectKey;
    use crate::domain::judgement::test_support::{approve, ca_passed, hash_of, jid, ts};
    use crate::domain::judgement::{FilePresence, Judgement, JudgementKind, Target};
    use crate::domain::keyring::PinKeyring;
    use crate::domain::location::SourceUri;
    use crate::domain::nonempty::NonBlank;

    fn texts(text: &CrossingText) -> Vec<(CrossingField, &str)> {
        text.pieces()
            .iter()
            .map(|p| (p.field(), p.text()))
            .collect()
    }

    fn agent() -> Judge {
        Judge::new(
            PersonName::new("Ada Marker").unwrap(),
            Some(Email::new("ada.marker@example.org").unwrap()),
            Principal::Agent {
                tool: ToolName::new("tool-marker").unwrap(),
            },
        )
    }

    fn judge_pieces() -> Vec<(CrossingField, &'static str)> {
        vec![
            (CrossingField::JudgeName, "Ada Marker"),
            (CrossingField::JudgeEmail, "ada.marker@example.org"),
            (CrossingField::JudgeTool, "tool-marker"),
        ]
    }

    // --- one test per type that crosses -------------------------------------

    #[test]
    fn a_judge_crosses_with_name_mail_and_tool() {
        let text = CrossingText::of(&agent());
        assert_eq!(texts(&text), judge_pieces());
        let human = Judge::new(PersonName::new("Ada").unwrap(), None, Principal::Human);
        assert_eq!(
            texts(&CrossingText::of(&human)),
            vec![(CrossingField::JudgeName, "Ada")]
        );
    }

    #[test]
    fn a_source_crosses_as_its_uri_or_its_citation() {
        let uri: SourceUri = "https://tracker.example/issues/7".parse().unwrap();
        assert_eq!(
            texts(&CrossingText::of(&uri)),
            vec![(CrossingField::Source, "https://tracker.example/issues/7")]
        );
        let citation = SourceUri::Citation(NonBlank::new("Knuth, vol. 3, p. 12").unwrap());
        assert_eq!(
            texts(&CrossingText::of(&citation)),
            vec![(CrossingField::Source, "Knuth, vol. 3, p. 12")]
        );
    }

    #[test]
    fn a_path_an_object_and_a_document_cross_with_their_text() {
        let path: RelPath = "docs/cache.md".parse().unwrap();
        assert_eq!(
            texts(&CrossingText::of(&path)),
            vec![(CrossingField::Path, "docs/cache.md")]
        );
        let object = ObjectText::new("the full text".into());
        assert_eq!(object.key(), TextHash::of("the full text"));
        assert_eq!(
            texts(&CrossingText::of(&object)),
            vec![(CrossingField::Object, "the full text")]
        );
        let document = DocumentText::new("id: x".into(), "the body".into());
        assert_eq!(
            texts(&CrossingText::of(&document)),
            vec![
                (CrossingField::FrontMatter, "id: x"),
                (CrossingField::Body, "the body")
            ]
        );
        // A part with nothing in it is not a piece.
        let bare = DocumentText::new("id: x".into(), "  \n".into());
        assert_eq!(texts(&CrossingText::of(&bare)).len(), 1);
    }

    #[test]
    fn a_document_is_split_from_its_bytes_and_none_of_them_is_left_out() {
        let split = |bytes: &[u8]| DocumentText::from_bytes(bytes);
        assert_eq!(
            split(b"---\nid: x\n---\nthe body\n"),
            DocumentText::new("id: x\n".into(), "the body\n".into())
        );
        // CRLF fences, and a `---` inside the body.
        assert_eq!(
            split(b"---\r\nid: x\r\n---\r\na\n---\nb\n"),
            DocumentText::new("id: x\r\n".into(), "a\n---\nb\n".into())
        );
        // No fence, or one that is never closed, is all body.
        for bytes in [
            &b"just text\n"[..],
            b"---\nid: x\nno close\n",
            b"",
            b"x\n---\ny\n",
        ] {
            let text = String::from_utf8_lossy(bytes).into_owned();
            assert_eq!(split(bytes), DocumentText::new(String::new(), text));
        }
        // A byte that is not UTF-8 is read as a replacement, and the rest is kept.
        let lossy = split(b"---\nid: \xff\n---\nfinding/secret-plan");
        assert_eq!(
            lossy,
            DocumentText::new("id: \u{fffd}\n".into(), "finding/secret-plan".into())
        );
    }

    #[test]
    fn a_retirement_a_purge_and_a_revocation_cross_with_their_reason_and_judge() {
        let reason = NonBlank::new("moved to another tracker").unwrap();
        let mut expected = judge_pieces();
        expected.push((CrossingField::Reason, "moved to another tracker"));

        let retire = Judgement::retire(
            jid(1),
            ts(),
            agent(),
            Visibility::Private,
            Target::Doc(docid(1)),
            FilePresence::Absent,
            reason.clone(),
        )
        .unwrap();
        assert_eq!(texts(&CrossingText::of(&retire)), expected);

        let purge = Judgement::purge(
            jid(2),
            ts(),
            agent(),
            Visibility::Private,
            docid(1),
            TextHash::of("t"),
            reason.clone(),
        );
        assert_eq!(texts(&CrossingText::of(&purge)), expected);

        let revoke = Judgement::assemble(
            jid(3),
            ts(),
            agent(),
            Visibility::Private,
            JudgementKind::Revoke {
                targets: NonEmptyVec::new(jid(1), Vec::new()),
                reason,
            },
        );
        assert_eq!(texts(&CrossingText::of(&revoke)), expected);
    }

    #[test]
    fn a_publication_record_carries_only_its_judge() {
        let published = Judgement::assemble(
            jid(4),
            ts(),
            agent(),
            Visibility::Public,
            JudgementKind::Published {
                doc: docid(1),
                records: [jid(1)].into(),
            },
        );
        assert_eq!(texts(&CrossingText::of(&published)), judge_pieces());
    }

    #[test]
    fn an_approval_crosses_with_the_revision_or_the_hint_of_its_attestation() {
        // A CA approval names the revision of the code.
        let ca = approve(5, &agent(), Visibility::Private, ca_passed(31, "item"));
        let mut expected = judge_pieces();
        expected.push((CrossingField::Revision, "a1"));
        assert_eq!(texts(&CrossingText::of(&ca)), expected);

        // A measurement that found a gap hints at the item missing.
        let cm = Passed::for_test(Attestation::Cm {
            spec: Pinned::new(sid(11), hash_of("hyp")),
            cs: hash_of("cs").to_string().parse().unwrap(),
            result: CmResult::FailWiring {
                item_hint: NonBlank::new("the retry item").unwrap(),
            },
        });
        let cm = approve(6, &agent(), Visibility::Private, cm);
        let mut expected = judge_pieces();
        expected.push((CrossingField::ItemHint, "the retry item"));
        assert_eq!(texts(&CrossingText::of(&cm)), expected);

        // The other results, and the other gates, carry no prose.
        let cm_hyp = Passed::for_test(Attestation::Cm {
            spec: Pinned::new(sid(11), hash_of("hyp")),
            cs: hash_of("cs").to_string().parse().unwrap(),
            result: CmResult::FailHyp { finding: fid(1) },
        });
        let record = approve(7, &agent(), Visibility::Private, cm_hyp);
        assert_eq!(texts(&CrossingText::of(&record)), judge_pieces());
        let cr = approve(
            8,
            &agent(),
            Visibility::Private,
            crate::domain::judgement::test_support::cr_passed(1, "v1"),
        );
        assert_eq!(texts(&CrossingText::of(&cr)), judge_pieces());
    }

    #[test]
    fn a_patch_revision_crosses_as_its_base() {
        let patch = Revision::Patch {
            base: crate::domain::revision::RevisionId::from_validated("b2".into()).unwrap(),
            patch: hash_of("patch"),
        };
        assert_eq!(
            texts(&CrossingText::of(&patch)),
            vec![(CrossingField::Revision, "b2")]
        );
    }

    #[test]
    fn an_adoption_crosses_with_its_source() {
        let corpus = crate::domain::judgement::test_support::corpus_of(vec![
            crate::domain::judgement::test_support::Filed::public(Entity::Finding(finding(
                1, "body",
            ))),
        ]);
        let adoptable = adoption::check(AdoptionInput {
            corpus: &corpus,
            gate: Gate::Cr,
            target: Target::Doc(docid(1)),
            source: "https://tracker.example/issues/7",
            keyring: &PinKeyring::empty(),
            text: TextHash::of("the file as read\n"),
        })
        .unwrap();
        let object = ObjectKey::new(*adoptable.attestation().hash(), *adoptable.text());
        let record = Judgement::adopt(
            jid(9),
            ts(),
            agent(),
            Visibility::Private,
            adoptable,
            object,
        )
        .unwrap();
        let mut expected = judge_pieces();
        expected.push((CrossingField::Source, "https://tracker.example/issues/7"));
        assert_eq!(texts(&CrossingText::of(&record)), expected);
    }

    #[test]
    fn text_can_be_gathered_from_several_values() {
        let mut text = CrossingText::default();
        assert!(text.is_empty());
        text.include(&ObjectText::new("one".into()));
        text.include(&"p/q.md".parse::<RelPath>().unwrap());
        assert_eq!(
            texts(&text),
            vec![
                (CrossingField::Object, "one"),
                (CrossingField::Path, "p/q.md")
            ]
        );
    }

    // --- the leak check -----------------------------------------------------

    fn file(
        entity: Entity,
        alias: Option<&str>,
        visibility: Visibility,
        path: &str,
    ) -> (FileFacts, Reading) {
        // The bytes of two files of one entity are equal: that is what a copy is.
        let bytes = BytesHash::of(format!("{entity:?}").as_bytes());
        let header = Header::new(
            entity.id(),
            entity.kind(),
            alias.map(|a| a.parse().unwrap()),
            Default::default(),
            None,
        );
        let git = match visibility {
            Visibility::Public => GitPlace::Tracked,
            Visibility::Private => GitPlace::Ignored,
        };
        (
            FileFacts::new(
                Location::new(visibility, path.parse().unwrap(), Knowable::Known(git)),
                bytes,
            ),
            Reading::resolved(Document::new(header, entity).unwrap()),
        )
    }

    /// A private Finding (alias `finding/secret-plan`, title `slow login`), a private
    /// Design with one item, and a public Finding with an alias of its own.
    fn corpus() -> Corpus {
        let items = vec![(1, identified(31, "the item", &[]))];
        Corpus::build(
            [
                file(
                    Entity::Finding(finding(1, "body")),
                    Some("finding/secret-plan"),
                    Visibility::Private,
                    "f1.md",
                ),
                file(
                    Entity::Design(design(21, 11, items)),
                    Some("design/secret-design"),
                    Visibility::Private,
                    "d21.md",
                ),
                file(
                    Entity::Finding(finding(2, "open body")),
                    Some("finding/open"),
                    Visibility::Public,
                    "f2.md",
                ),
            ],
            &Default::default(),
        )
    }

    fn known(root: PrivateRoot, except: &[u128]) -> Knowable<PrivateNames> {
        let except: BTreeSet<DocId> = except.iter().map(|n| docid(*n)).collect();
        PrivateNames::of(&corpus(), root, &except)
    }

    fn crossing(text: &str) -> CrossingText {
        CrossingText::of(&ObjectText::new(text.to_owned()))
    }

    #[test]
    fn the_private_root_decides_what_can_be_known() {
        assert_eq!(known(PrivateRoot::Absent, &[]), Knowable::Unknown);
        assert_eq!(
            known(PrivateRoot::NotUsedHere, &[]),
            Knowable::Known(PrivateNames::default())
        );
        let present = known(PrivateRoot::Present, &[]);
        let names = present.known().unwrap();
        assert!(!names.is_empty());
    }

    #[test]
    fn a_leak_of_an_alias_a_title_or_an_unmarked_id_is_found_with_its_field() {
        let id = docid(1).to_string();
        let item = iid(31).to_string();
        let text = format!(
            "see finding/secret-plan, the slow login finding, {id} and the item {item}\n\
             but not finding/open or private:{id}"
        );
        let names = known(PrivateRoot::Present, &[]);
        let Err(LeakRefusal::Leaks(leaks)) = check_crossing(&crossing(&text), &names) else {
            panic!("expected leaks");
        };
        let found: Vec<&LeakFinding> = leaks.iter().map(Leak::finding).collect();
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(
            leaks
                .iter()
                .all(|leak| leak.field() == CrossingField::Object)
        );
        assert!(found.iter().any(|f| matches!(
            f.name(),
            Some(PrivateName::Alias(a)) if a.as_str() == "finding/secret-plan"
        )));
        assert!(found.iter().any(|f| matches!(
            f.name(),
            Some(PrivateName::Title(t)) if t.as_str() == "slow login"
        )));
        assert_eq!(
            found.iter().filter_map(|f| f.unmarked_reference()).count(),
            2
        );
    }

    #[test]
    fn the_document_made_public_may_carry_its_own_names() {
        let text = "finding/secret-plan is slow login, id ".to_owned() + &docid(1).to_string();
        let names = known(PrivateRoot::Present, &[1]);
        assert_eq!(check_crossing(&crossing(&text), &names), Ok(()));
        // Its neighbours are still private.
        let other = known(PrivateRoot::Present, &[1]);
        assert!(matches!(
            check_crossing(&crossing("design/secret-design"), &other),
            Err(LeakRefusal::Leaks(_))
        ));
    }

    #[test]
    fn nothing_is_checked_where_no_private_root_is_used_and_nothing_is_written_where_it_is_unreadable()
     {
        let text = crossing("finding/secret-plan");
        assert_eq!(
            check_crossing(&text, &known(PrivateRoot::NotUsedHere, &[])),
            Ok(())
        );
        assert_eq!(
            check_crossing(&text, &known(PrivateRoot::Absent, &[])),
            Err(LeakRefusal::Uncheckable)
        );
    }

    #[test]
    fn every_piece_is_checked_and_a_clean_text_passes() {
        let mut text = CrossingText::default();
        text.include(&DocumentText::new(
            "alias: finding/secret-plan".into(),
            "a clean body".into(),
        ));
        text.include(&format!("docs/{}.md", docid(1)).parse::<RelPath>().unwrap());
        let names = known(PrivateRoot::Present, &[]);
        let Err(LeakRefusal::Leaks(leaks)) = check_crossing(&text, &names) else {
            panic!("expected leaks");
        };
        let fields: Vec<CrossingField> = leaks.iter().map(Leak::field).collect();
        assert_eq!(
            fields,
            vec![CrossingField::FrontMatter, CrossingField::Path]
        );
        assert_eq!(check_crossing(&crossing("a clean text"), &names), Ok(()));
        assert_eq!(check_crossing(&CrossingText::default(), &names), Ok(()));
    }

    #[test]
    fn a_path_built_from_a_private_alias_is_a_leak_and_prose_is_read_as_before() {
        let names = known(PrivateRoot::Present, &[]);
        let path = |text: &str| CrossingText::of(&text.parse::<RelPath>().unwrap());
        for leaky in [
            "design/secret-design.md",
            "docs/design/secret-design.md",
            "design/secret-design/notes.md",
            "design/secret-design.tar.gz",
            "docs/finding/secret-plan",
        ] {
            let Err(LeakRefusal::Leaks(leaks)) = check_crossing(&path(leaky), &names) else {
                panic!("expected a leak in {leaky}");
            };
            assert!(
                leaks.iter().all(|leak| leak.field() == CrossingField::Path),
                "{leaky}"
            );
            assert_eq!(leaks.len(), 1, "{leaky}: {leaks:?}");
        }
        // A name that only starts or ends alike is another name.
        for clean in [
            "design/secret-design-notes.md",
            "design/secret-designs.md",
            "old-design/secret-design.md",
            "docs/cache.md",
        ] {
            assert_eq!(check_crossing(&path(clean), &names), Ok(()), "{clean}");
        }
        // In prose, a dot followed by more of a token still continues it, as before.
        assert_eq!(
            check_crossing(&crossing("see design/secret-design.md"), &names),
            Ok(())
        );
    }

    #[test]
    fn the_name_of_the_key_of_a_keyed_pin_crosses() {
        let kid: crate::domain::keyring::Kid = "laptop-key".parse().unwrap();
        let key = crate::domain::hash::PinKey::new(&[7; 32]).unwrap();
        let mac = crate::domain::hash::PinMac::of(&key, iid(31).entity(), &hash_of("d"));
        let keyed = crate::domain::keyring::UpstreamHash::Keyed(
            crate::domain::keyring::KeyedPin::new(kid, mac),
        );
        assert_eq!(
            texts(&CrossingText::of(&keyed)),
            vec![(CrossingField::KeyName, "laptop-key")]
        );
        let plain = crate::domain::keyring::UpstreamHash::Plain(hash_of("d"));
        assert!(CrossingText::of(&plain).is_empty());
    }

    #[test]
    fn a_publication_not_recorded_yet_still_counts_as_private() {
        // The public file is there and `Published` is not: the document is private yet.
        let entity = Entity::Finding(finding(1, "body"));
        let public = file(
            entity.clone(),
            Some("finding/half"),
            Visibility::Public,
            "p.md",
        );
        let private = file(entity, Some("finding/half"), Visibility::Private, "q.md");
        let corpus = Corpus::build([public, private], &BTreeSet::new());
        let known = PrivateNames::of(&corpus, PrivateRoot::Present, &BTreeSet::new());
        assert!(!known.known().unwrap().is_empty());
        assert!(corpus.publishing_of(&docid(1)).is_some());
    }
}
