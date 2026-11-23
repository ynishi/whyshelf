//! The words of the layout rustdoc writes, and the place in it a path comes to.
//!
//! | what | what it is |
//! |---|---|
//! | [`ITEM_KINDS`], [`TYPE_KINDS`] | the kinds of item that have a page `<kind>.<Name>.html`, and those of them whose page has members |
//! | [`MemberKind`], [`MEMBER_KINDS`] | the kinds of member that have an anchor in the page of a type, from the one table of them |
//! | [`MODULE_KIND`], [`GENERATOR_MARK`] | the kind of the place of a module, and the mark of a page rustdoc wrote |
//! | [`Place`] | where in the docs the thing a path names is told of |
//!
//! This module reads no file and depends on no sibling but the name of a place in the
//! docs (`docs_path`): what reads the layout (`lookup`, `promise`) takes its words from
//! here, so a change of the layout is a change of this module's words.

use super::docs_path::DocsPath;

/// The mark rustdoc writes in the head of each page: an index without it is no
/// docs this adapter reads.
pub const GENERATOR_MARK: &str = "<meta name=\"generator\" content=\"rustdoc\">";

/// The kind of the place of a module, and of a crate (its index).
pub(super) const MODULE_KIND: &str = "module";

/// The kinds of item rustdoc writes a page `<kind>.<Name>.html` for.
pub const ITEM_KINDS: &[&str] = &[
    "struct",
    "enum",
    "union",
    "trait",
    "traitalias",
    "fn",
    "type",
    "constant",
    "static",
    "macro",
    "attr",
    "derive",
    "primitive",
    "keyword",
    "foreigntype",
];

/// The kinds of item whose page has members a path can name.
pub(super) const TYPE_KINDS: &[&str] = &[
    "struct",
    "enum",
    "union",
    "trait",
    "traitalias",
    "primitive",
    "foreigntype",
];

/// The kinds of member rustdoc writes an anchor `id="<kind>.<name>"` for in the page
/// of a type, closed: the one table of them and of their anchor words is
/// [`MemberKind::TABLE`], in the order a name is looked up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberKind {
    /// A method with a body, of an impl.
    Method,
    /// A method of a trait, with no body.
    TyMethod,
    /// A variant of an enum.
    Variant,
    /// A field of a struct or a union.
    StructField,
    /// An associated constant.
    AssociatedConstant,
    /// An associated type.
    AssociatedType,
}

impl MemberKind {
    /// Every kind with the word of its anchor (`method` in `id="method.new"`), the
    /// one table of them: a name is looked up in this order.
    const TABLE: [(Self, &'static str); 6] = [
        (Self::Method, "method"),
        (Self::TyMethod, "tymethod"),
        (Self::Variant, "variant"),
        (Self::StructField, "structfield"),
        (Self::AssociatedConstant, "associatedconstant"),
        (Self::AssociatedType, "associatedtype"),
    ];

    /// The word of the anchor of this kind.
    pub(super) fn word(self) -> &'static str {
        Self::TABLE
            .iter()
            .find(|(kind, _)| *kind == self)
            .map_or("", |(_, word)| word)
    }

    /// The kinds in the order a name is looked up in.
    pub(super) fn all() -> impl Iterator<Item = Self> {
        Self::TABLE.iter().map(|(kind, _)| *kind)
    }

    /// The anchor words of [`Self::TABLE`], in its order.
    const fn words() -> [&'static str; 6] {
        let mut words = [""; 6];
        let mut at = 0;
        while at < words.len() {
            words[at] = Self::TABLE[at].1;
            at += 1;
        }
        words
    }
}

/// The words of the anchors of the kinds of member rustdoc writes, made from the one
/// table of the kinds of member: the lookup of a member reads the kinds in this order.
pub const MEMBER_KINDS: &[&str] = &MemberKind::words();

/// Where in the docs the thing a path names is told of: the one place
/// `Lookup::find` hands over, for what its promise is read from. Only a member has
/// an anchor, so it is the one variant that has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Place {
    /// A module, or a crate: the index of its directory ([`MODULE_KIND`]).
    Module {
        /// The `index.html` of the module.
        page: DocsPath,
    },
    /// An item: its page.
    Item {
        page: DocsPath,
        /// The kind of the page: a kind of [`ITEM_KINDS`].
        kind: String,
    },
    /// A member of a type: the page of the type, the kind of the member and the
    /// `id` of its element on the page (`method.new`).
    Member {
        page: DocsPath,
        kind: MemberKind,
        anchor: String,
    },
}

impl Place {
    /// The page the promise is read from.
    pub(super) fn page(&self) -> &DocsPath {
        match self {
            Self::Module { page } | Self::Item { page, .. } | Self::Member { page, .. } => page,
        }
    }
}
