//! What an item promises: its declaration and its docs, read from the page rustdoc
//! wrote for it, as the one text a link of the preset `rustdoc-promise` compares.
//!
//! | what | what it does |
//! |---|---|
//! | `promise_of` | the one reader of a page: the promise of the [`Place`] in it, or why it cannot be read |
//! | `Promise::text` | the one place the text is formed, which `resolve` hashes and `current` returns |
//! | `has_declaration` | the one table of the kinds that have a declaration |
//!
//! ```text
//!   page + Place ──► the part of the page the place owns
//!                      item    : the declaration (`item-decl`) and the top doc
//!                                (the declaration lists a struct's public fields, an
//!                                enum's variants, a trait's item signatures, a
//!                                const's value)
//!                      member  : its code header, the docblock right after it
//!                                (a variant: and the fields that follow)
//!                 ──► Promise::Declared { declaration, docs }   a kind with a declaration
//!                     Promise::DocsOnly(docs)                   module, crate, primitive, keyword
//!                     Unreadable                                no declaration where one is
//!                                                               written, a layout not read
//! ```
//!
//! The text of a promise is UTF-8: each line of the declaration starts with `decl `,
//! each of the docs with `doc `, the declaration first. A line with nothing in it
//! (or only white space) is written empty, so what a line starts with tells a
//! declaration from the docs, whatever a line of the docs says. The text has the
//! line breaks rustdoc wrote, and the adapter collapses no white space: whether
//! white space and blank lines count is the comparison's to say (`lines` collapses
//! white space and leaves out blank lines, `bytes` does neither).
//!
//! # Reading a page
//!
//! rustdoc's HTML is no contract, and the layout read here is the one rustdoc 1.95
//! writes. Tags are read with both `"` and `'` as the quote of an attribute; the
//! text of a part is its HTML with the tags taken out and the entities (the named
//! ones rustdoc writes and the numeric ones) put back. A block tag (`p`, `li`, a
//! heading, `br`, `tr`, `td`, `div`, `pre`, ...) ends a line, and so does each line
//! break inside the text. The link `Source`, the anchors `§` of a heading and the
//! `Expand description` of a toggle are rustdoc's, not the item's, and are left out;
//! the mark of a deprecated item or of a feature (`span.item-info`) is left out too.

use super::{MODULE_KIND, MemberKind, Place};

/// What an item promises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Promise {
    /// The kinds that have a declaration (see [`has_declaration`]).
    Declared {
        /// What it declares.
        declaration: Declaration,
        /// What its docs say.
        docs: Docs,
    },
    /// A module, a crate, a primitive type and a keyword: they have docs, and no
    /// declaration.
    DocsOnly(Docs),
}

/// The text of a declaration, with the line breaks rustdoc wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Declaration(String);

/// The docs of an item, as text. No docs is a promise too: an item that has none
/// promises nothing in words, and writing some is a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Docs {
    /// The text of the docs.
    Written(String),
    /// There are none.
    Absent,
}

impl Docs {
    /// The docs these lines make: none when there is nothing in them.
    fn of(text: String) -> Self {
        if text.trim().is_empty() {
            Self::Absent
        } else {
            Self::Written(text)
        }
    }
}

/// Why the promise of an item cannot be read from its page.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(super) enum Unreadable {
    /// The kind has a declaration, and the page has none where rustdoc writes it.
    #[error(
        "the page of the item has no declaration where rustdoc writes one, so what it promises cannot be told"
    )]
    NoDeclaration,
    /// The page is not laid out as this adapter reads it.
    #[error("the page of the item is not laid out as this adapter reads it: {0}")]
    Layout(&'static str),
    /// The page cannot be read, or is too large to be.
    #[error("{0}")]
    Page(String),
}

/// The kinds that have no declaration of their own on their page, the one table of
/// them: the page of a module (a crate is one), of a primitive type and of a
/// keyword has docs only. Every other kind rustdoc writes (an item, a member, an
/// attribute macro, a derive macro) has one.
const NO_DECLARATION: &[&str] = &[MODULE_KIND, "primitive", "keyword"];

/// Whether a page of this kind has a declaration.
fn has_declaration(kind: &str) -> bool {
    !NO_DECLARATION.contains(&kind)
}

impl Promise {
    /// The text of the promise: the one place it is formed. The lines of the
    /// declaration come first, each after `decl `; then those of the docs, each
    /// after `doc `. An empty line is written empty, and every line ends with `\n`.
    pub(super) fn text(&self) -> String {
        let (declaration, docs) = match self {
            Self::Declared { declaration, docs } => (Some(declaration), docs),
            Self::DocsOnly(docs) => (None, docs),
        };
        let mut text = String::new();
        if let Some(Declaration(lines)) = declaration {
            push_lines(&mut text, "decl", lines);
        }
        if let Docs::Written(lines) = docs {
            push_lines(&mut text, "doc", lines);
        }
        text
    }
}

/// Each line of `lines` after `word`, or empty when it has nothing in it.
fn push_lines(text: &mut String, word: &str, lines: &str) {
    for line in lines.split('\n') {
        if !line.trim().is_empty() {
            text.push_str(word);
            text.push(' ');
            text.push_str(line);
        }
        text.push('\n');
    }
}

/// The promise of the item or the member `place` names, read from the HTML `html`
/// of its page.
pub(super) fn promise_of(html: &str, place: &Place) -> Result<Promise, Unreadable> {
    let main = main_content(html).ok_or(Unreadable::Layout("the page has no main content"))?;
    // A member always has a declaration; a module and an item have one by their kind.
    let (parts, declared) = match place {
        Place::Module { .. } => (item_parts(main)?, has_declaration(MODULE_KIND)),
        Place::Item { kind, .. } => (item_parts(main)?, has_declaration(kind)),
        Place::Member { kind, anchor, .. } => (member_parts(main, anchor, *kind)?, true),
    };
    if declared {
        let declaration = parts.declaration.ok_or(Unreadable::NoDeclaration)?;
        Ok(Promise::Declared {
            declaration: Declaration(declaration),
            docs: parts.docs,
        })
    } else {
        Ok(Promise::DocsOnly(parts.docs))
    }
}

/// What a page tells of its item or member, before the kind decides what a promise
/// is.
struct Parts {
    /// The text of the declaration, when the page has one.
    declaration: Option<String>,
    docs: Docs,
}

const NOT_CLOSED: Unreadable = Unreadable::Layout("an element of the page is not closed");

/// The page from its main content on (the sidebar and the head are not read).
fn main_content(html: &str) -> Option<&str> {
    let at = ["id=\"main-content\"", "id='main-content'"]
        .iter()
        .filter_map(|mark| html.find(mark))
        .min()?;
    let start = html[..at].rfind('<')?;
    Some(&html[start..])
}

/// The declaration (`pre.item-decl`) and the top doc (`details.top-doc`) of the
/// page of an item. The top doc is read only before the first heading of the page:
/// a page that has none has no docs, and what comes after is another's.
fn item_parts(main: &str) -> Result<Parts, Unreadable> {
    let mut lexer = Lexer::new(main);
    let mut declaration = None;
    let mut docs = Docs::Absent;
    while let Some(next) = lexer.next() {
        let Token::Open(tag) = next.token else {
            continue;
        };
        if tag.is("pre") && tag.has_class("item-decl") && declaration.is_none() {
            let block = lexer.inner(&tag).ok_or(NOT_CLOSED)?;
            declaration = Some(render(block.of(main)));
        } else if tag.is("details") && tag.has_class("top-doc") {
            if let Some(block) = lexer.docblock_in(&tag) {
                docs = Docs::of(render(block.of(main)));
            }
            break;
        } else if tag.is("h2") {
            break;
        }
    }
    Ok(Parts {
        declaration: declaration.filter(|text| !text.trim().is_empty()),
        docs,
    })
}

/// The code header of the member whose element has the id `anchor`, and what
/// follows it: the docblock right after the element (inside the same toggle, when
/// it has one), and for a variant the list of its fields after that.
fn member_parts(main: &str, anchor: &str, kind: MemberKind) -> Result<Parts, Unreadable> {
    let mut lexer = Lexer::new(main);
    let element = loop {
        let Some(next) = lexer.next() else {
            return Err(Unreadable::Layout("the member is not on its page"));
        };
        if let Token::Open(tag) = next.token
            && tag.attr("id") == Some(anchor)
        {
            break tag;
        }
    };
    let element = lexer.inner(&element).ok_or(NOT_CLOSED)?;
    let declaration = header_in(element.of(main), kind).filter(|text| !text.trim().is_empty());
    // Only a variant lists fields under it.
    let lists_fields = match kind {
        MemberKind::Variant => true,
        MemberKind::Method
        | MemberKind::TyMethod
        | MemberKind::StructField
        | MemberKind::AssociatedConstant
        | MemberKind::AssociatedType => false,
    };
    let (docblock, fields) = trailing(main, element.after, lists_fields);
    let docs: Vec<String> = [docblock, fields]
        .into_iter()
        .flatten()
        .map(|block| render(block.of(main)))
        .filter(|text| !text.trim().is_empty())
        .collect();
    Ok(Parts {
        declaration,
        docs: Docs::of(docs.join("\n")),
    })
}

/// The text of what a member declares inside its element: the code header of a
/// method, a variant and the like; the `code` of a field.
fn header_in(element: &str, kind: MemberKind) -> Option<String> {
    let mut lexer = Lexer::new(element);
    while let Some(next) = lexer.next() {
        let Token::Open(tag) = next.token else {
            continue;
        };
        let is_header = match kind {
            MemberKind::StructField => tag.is("code"),
            MemberKind::Method
            | MemberKind::TyMethod
            | MemberKind::Variant
            | MemberKind::AssociatedConstant
            | MemberKind::AssociatedType => tag.has_class("code-header"),
        };
        if is_header {
            let block = lexer.inner(&tag)?;
            return Some(render(block.of(element)));
        }
    }
    None
}

/// What follows the element of a member, from the offset `from`: the docblock when
/// it is the next sibling (after the end of a summary, when the member is the
/// summary of a toggle, and after the mark of a deprecated item), then, when
/// `fields` is asked for, the list of the fields of a variant.
fn trailing(main: &str, from: usize, fields: bool) -> (Option<Block>, Option<Block>) {
    let mut lexer = Lexer::from(main, from);
    let mut docblock = None;
    let mut list = None;
    while let Some(next) = lexer.next() {
        match next.token {
            Token::Text(text) if text.trim().is_empty() => {}
            Token::Close(name) if same(name, "summary") => {}
            Token::Open(tag) if tag.is("span") && tag.has_class("item-info") => {
                if lexer.inner(&tag).is_none() {
                    break;
                }
            }
            Token::Open(tag)
                if tag.is("div") && tag.has_class("docblock") && docblock.is_none() =>
            {
                docblock = lexer.inner(&tag);
                if docblock.is_none() || !fields {
                    break;
                }
            }
            Token::Open(tag) if fields && tag.is("div") && tag.has_class("sub-variant") => {
                list = lexer.inner(&tag);
                break;
            }
            _ => break,
        }
    }
    (docblock, list)
}

/// The text of a part of a page: the tags are taken out, the entities put back, and
/// a block tag or a line break of the text ends a line. Leading and trailing blank
/// lines are left out; nothing else is changed.
fn render(html: &str) -> String {
    let mut lines = Lines::default();
    let mut lexer = Lexer::new(html);
    while let Some(next) = lexer.next() {
        match next.token {
            Token::Text(text) => lines.text(&decode(text)),
            Token::Open(tag) => {
                let own = (tag.is("a") && (tag.has_class("anchor") || tag.has_class("doc-anchor")))
                    || tag.is("script")
                    || tag.is("style");
                if own {
                    // Rustdoc's own: the `§` of an anchor, and no text of the item.
                    let _ = lexer.inner(&tag);
                } else if breaks(tag.name) {
                    lines.soft_break();
                }
            }
            Token::Close(name) => {
                if breaks(name) {
                    lines.soft_break();
                }
            }
        }
    }
    lines.finish()
}

/// The tags that end a line.
const BLOCKS: &[&str] = &[
    "p",
    "li",
    "ul",
    "ol",
    "dl",
    "dt",
    "dd",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "br",
    "hr",
    "tr",
    "td",
    "th",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "div",
    "pre",
    "blockquote",
    "details",
    "summary",
    "section",
];

fn breaks(name: &str) -> bool {
    BLOCKS.iter().any(|block| same(name, block))
}

/// The lines of a text, as it is read.
#[derive(Default)]
struct Lines {
    done: Vec<String>,
    current: String,
}

impl Lines {
    /// Text: each line break in it ends a line.
    fn text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        for ch in text.chars() {
            if ch == '\n' {
                self.done.push(std::mem::take(&mut self.current));
            } else {
                self.current.push(ch);
            }
        }
    }

    /// A block tag: ends the line it is in, unless that has nothing in it.
    fn soft_break(&mut self) {
        if !self.current.is_empty() {
            self.done.push(std::mem::take(&mut self.current));
        }
    }

    /// The lines, without the blank ones at either end, one text.
    fn finish(mut self) -> String {
        self.soft_break();
        let blank = |line: &String| line.trim().is_empty();
        let first = self.done.iter().position(|line| !blank(line));
        let last = self.done.iter().rposition(|line| !blank(line));
        match (first, last) {
            (Some(first), Some(last)) => self.done[first..=last].join("\n"),
            _ => String::new(),
        }
    }
}

/// The text with the entities put back: the named ones rustdoc writes and the
/// numeric ones (`&#39;`, `&#x27;`). One that is none is left as written.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        match entity(tail) {
            Some((ch, len)) => {
                out.push(ch);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character the entity at the start of `tail` (which starts with `&`) writes,
/// and the length of the entity.
fn entity(tail: &str) -> Option<(char, usize)> {
    let end = tail
        .char_indices()
        .take(12)
        .find_map(|(at, ch)| (ch == ';').then_some(at))?;
    let name = &tail[1..end];
    let ch = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => {
            let digits = name.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((ch, end + 1))
}

/// Whether two tag names are the same tag.
fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// A tag that opens an element.
#[derive(Debug, Clone, Copy)]
struct Tag<'a> {
    name: &'a str,
    /// What is between the name and the `>`.
    attrs: &'a str,
}

impl<'a> Tag<'a> {
    fn is(&self, name: &str) -> bool {
        same(self.name, name)
    }

    /// The value of an attribute, quoted with `"` or `'` or not quoted.
    fn attr(&self, name: &str) -> Option<&'a str> {
        let mut rest = self.attrs;
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
            if rest.is_empty() {
                return None;
            }
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '=' || c == '/')
                .unwrap_or(rest.len());
            let (key, after) = rest.split_at(end);
            let after = after.trim_start();
            let (value, next) = match after.strip_prefix('=') {
                None => ("", after),
                Some(value) => {
                    let value = value.trim_start();
                    match value.chars().next() {
                        Some(quote @ ('"' | '\'')) => {
                            let body = &value[1..];
                            match body.find(quote) {
                                Some(close) => (&body[..close], &body[close + 1..]),
                                None => (body, ""),
                            }
                        }
                        _ => {
                            let close = value.find(char::is_whitespace).unwrap_or(value.len());
                            value.split_at(close)
                        }
                    }
                }
            };
            if key.eq_ignore_ascii_case(name) {
                return Some(value);
            }
            rest = next;
        }
    }

    fn has_class(&self, class: &str) -> bool {
        self.attr("class")
            .is_some_and(|classes| classes.split_whitespace().any(|c| c == class))
    }
}

/// A piece of HTML.
enum Token<'a> {
    Open(Tag<'a>),
    Close(&'a str),
    Text(&'a str),
}

/// A token and where it is in the HTML.
struct Spanned<'a> {
    token: Token<'a>,
    start: usize,
    end: usize,
}

/// The content of an element, as offsets in the HTML read.
#[derive(Debug, Clone, Copy)]
struct Block {
    start: usize,
    end: usize,
    /// Where the element ends, its closing tag included.
    after: usize,
}

impl Block {
    fn of<'a>(&self, html: &'a str) -> &'a str {
        &html[self.start..self.end]
    }
}

/// HTML, token by token. It knows only what these pages need: tags with quoted
/// attributes, the text between them, and comments and declarations (skipped).
struct Lexer<'a> {
    html: &'a str,
    at: usize,
}

impl<'a> Lexer<'a> {
    fn new(html: &'a str) -> Self {
        Self::from(html, 0)
    }

    fn from(html: &'a str, at: usize) -> Self {
        Self { html, at }
    }

    fn next(&mut self) -> Option<Spanned<'a>> {
        loop {
            let start = self.at;
            let rest = self.html.get(start..).filter(|rest| !rest.is_empty())?;
            let (token, len) = match markup(rest) {
                Markup::Skip(len) => {
                    self.at = start + len;
                    continue;
                }
                Markup::Token(token, len) => (token, len),
            };
            self.at = start + len;
            return Some(Spanned {
                token,
                start,
                end: self.at,
            });
        }
    }

    /// The content of the element `tag`, which the lexer has just read the opening
    /// of: the lexer goes on after its closing tag. None when it is never closed.
    fn inner(&mut self, tag: &Tag<'_>) -> Option<Block> {
        let start = self.at;
        let mut depth = 1_usize;
        while let Some(next) = self.next() {
            match next.token {
                Token::Open(open) if open.is(tag.name) => depth += 1,
                Token::Close(name) if same(name, tag.name) => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(Block {
                            start,
                            end: next.start,
                            after: next.end,
                        });
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The first docblock inside the toggle `toggle` the lexer has just read the
    /// opening of, none when the toggle ends before one.
    fn docblock_in(&mut self, toggle: &Tag<'_>) -> Option<Block> {
        while let Some(next) = self.next() {
            match next.token {
                Token::Open(tag) if tag.is("div") && tag.has_class("docblock") => {
                    return self.inner(&tag);
                }
                Token::Close(name) if same(name, toggle.name) => return None,
                _ => {}
            }
        }
        None
    }
}

enum Markup<'a> {
    /// Nothing to read: a comment or a declaration, so long.
    Skip(usize),
    Token(Token<'a>, usize),
}

/// The markup or the text at the start of `rest` (not empty).
fn markup(rest: &str) -> Markup<'_> {
    if !starts_markup(rest) {
        let len = text_len(rest);
        return Markup::Token(Token::Text(&rest[..len]), len);
    }
    if let Some(after) = rest.strip_prefix("<!--") {
        let len = after
            .find("-->")
            .map_or(rest.len(), |at| "<!--".len() + at + "-->".len());
        return Markup::Skip(len);
    }
    if rest.starts_with("<!") || rest.starts_with("<?") {
        return Markup::Skip(rest.find('>').map_or(rest.len(), |at| at + 1));
    }
    if let Some(after) = rest.strip_prefix("</") {
        let name_len = after
            .find(['>', ' ', '\t', '\n', '\r'])
            .unwrap_or(after.len());
        let len = rest.find('>').map_or(rest.len(), |at| at + 1);
        return Markup::Token(Token::Close(&after[..name_len]), len);
    }
    let after = &rest[1..];
    let name_len = after
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(after.len());
    let mut quote: Option<char> = None;
    for (at, ch) in after[name_len..].char_indices() {
        match (quote, ch) {
            (Some(open), ch) if ch == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '>') => {
                let attrs = &after[name_len..name_len + at];
                let tag = Tag {
                    name: &after[..name_len],
                    attrs,
                };
                return Markup::Token(Token::Open(tag), 1 + name_len + at + 1);
            }
            (None, _) => {}
        }
    }
    // A tag that never ends is text.
    Markup::Token(Token::Text(rest), rest.len())
}

/// Whether `rest` (not empty) starts a tag, a closing tag, a comment or a
/// declaration, rather than text.
fn starts_markup(rest: &str) -> bool {
    let mut chars = rest.chars();
    if chars.next() != Some('<') {
        return false;
    }
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() => true,
        Some('!' | '?') => true,
        Some('/') => chars.next().is_some_and(|ch| ch.is_ascii_alphabetic()),
        _ => false,
    }
}

/// The length of the text at the start of `rest`, up to the next markup.
fn text_len(rest: &str) -> usize {
    let mut from = usize::from(rest.starts_with('<'));
    while let Some(at) = rest[from..].find('<') {
        let at = from + at;
        if starts_markup(&rest[at..]) {
            return at;
        }
        from = at + 1;
    }
    rest.len()
}

#[cfg(test)]
mod tests {
    //! The pages in these tests are written by hand, in the layout rustdoc 1.95
    //! writes (the classes and the order of the elements), with words of this
    //! repository only: no page rustdoc wrote, and no text of the standard library,
    //! is copied into one. The end-to-end tests run `cargo doc` on a crate of their
    //! own and watch the layout of the real one.

    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use whyshelf_core::domain::shelf::artifact::Comparison;

    use super::*;

    fn place(kind: &str, anchor: Option<&str>) -> Place {
        let page = PathBuf::from("page.html");
        match anchor {
            Some(anchor) => Place::Member {
                page,
                kind: MemberKind::all()
                    .find(|member| member.word() == kind)
                    .expect("a kind of member"),
                anchor: anchor.to_owned(),
            },
            None if kind == "module" => Place::Module { page },
            None => Place::Item {
                page,
                kind: kind.to_owned(),
            },
        }
    }

    /// A page with this main content, and a sidebar with a heading before it.
    fn page(main: &str) -> String {
        format!(
            "<!DOCTYPE html><html><head><meta name=\"generator\" content=\"rustdoc\"></head><body><nav class=\"sidebar\"><h2 class=\"location\"><a href=\"#\">Shelf</a></h2></nav><main><div class=\"width-limiter\"><section id=\"main-content\" class=\"content\" tabindex=\"-1\">{main}</section></div></main></body></html>"
        )
    }

    /// The head of an item's page, with the line `Source` and a declaration.
    fn heading(kind: &str, name: &str) -> String {
        format!(
            "<div class=\"main-heading\"><h1>{kind} <span class=\"struct\">{name}</span>&nbsp;<button id=\"copy-path\">Copy item path</button></h1><span class=\"sub-heading\"><a class=\"src\" href=\"../src/demo/lib.rs.html#1-2\">Source</a> </span></div>"
        )
    }

    const SHELF_DECL: &str = "<pre class=\"rust item-decl\"><code>pub struct Shelf {\n    pub root: <a class=\"struct\" href=\"x.html\" title=\"struct alloc::string::String\">String</a>,\n}</code></pre>";

    const SHELF_DOCS: &str = "<details class=\"toggle top-doc\" open><summary class=\"hideme\"><span>Expand description</span></summary><div class=\"docblock\"><p>A shelf of books.</p>\n<p>It holds a <em>root</em> &amp; more.</p>\n</div></details>";

    fn shelf(decl: &str, docs: &str, rest: &str) -> String {
        page(&format!("{}{decl}{docs}{rest}", heading("Struct", "Shelf")))
    }

    fn promise(html: &str, kind: &str, anchor: Option<&str>) -> Result<Promise, Unreadable> {
        promise_of(html, &place(kind, anchor))
    }

    fn text(html: &str, kind: &str, anchor: Option<&str>) -> String {
        promise(html, kind, anchor).unwrap().text()
    }

    fn lines_hash(text: &str) -> String {
        let lines = Comparison::Lines {
            ignore: BTreeSet::new(),
        };
        lines.fingerprint(text.as_bytes()).to_string()
    }

    fn bytes_hash(text: &str) -> String {
        Comparison::Bytes.fingerprint(text.as_bytes()).to_string()
    }

    #[test]
    fn an_item_promises_its_declaration_and_its_docs() {
        let html = shelf(SHELF_DECL, SHELF_DOCS, "");
        let promise = promise(&html, "struct", None).unwrap();
        assert_eq!(
            promise,
            Promise::Declared {
                declaration: Declaration("pub struct Shelf {\n    pub root: String,\n}".to_owned()),
                docs: Docs::Written("A shelf of books.\n\nIt holds a root & more.".to_owned()),
            }
        );
        assert_eq!(
            promise.text(),
            "decl pub struct Shelf {\ndecl     pub root: String,\ndecl }\ndoc A shelf of books.\n\ndoc It holds a root & more.\n"
        );
    }

    #[test]
    fn an_item_without_docs_promises_its_declaration_and_no_docs() {
        // No top doc, and the docs of a member after it are not the item's.
        let rest = "<h2 id=\"fields\" class=\"fields section-header\">Fields<a href=\"#fields\" class=\"anchor\">§</a></h2><span id=\"structfield.root\" class=\"structfield section-header\"><code>root: u8</code></span><div class=\"docblock\"><p>The root.</p>\n</div>";
        let html = shelf(SHELF_DECL, "", rest);
        let promise = promise(&html, "struct", None).unwrap();
        assert_eq!(
            promise,
            Promise::Declared {
                declaration: Declaration("pub struct Shelf {\n    pub root: String,\n}".to_owned()),
                docs: Docs::Absent,
            }
        );
        assert_eq!(
            promise.text(),
            "decl pub struct Shelf {\ndecl     pub root: String,\ndecl }\n"
        );
    }

    #[test]
    fn what_is_not_the_item_is_not_in_its_promise() {
        let plain = shelf(SHELF_DECL, SHELF_DOCS, "");
        // Another body (a list of impls), a deprecation mark, and the line `Source`
        // that goes elsewhere.
        let other = page(&format!(
            "{}{SHELF_DECL}<span class=\"item-info\"><div class=\"stab deprecated\"><span>Deprecated: no</span></div></span>{SHELF_DOCS}<h2 id=\"implementations\" class=\"section-header\">Implementations<a href=\"#implementations\" class=\"anchor\">§</a></h2><div id=\"implementations-list\"><section id=\"impl-Shelf\" class=\"impl\"><h3 class=\"code-header\">impl Shelf</h3></section></div>",
            heading("Struct", "Shelf").replace("#1-2", "#40-90")
        ));
        assert_eq!(text(&plain, "struct", None), text(&other, "struct", None));
    }

    #[test]
    fn a_change_of_the_docs_the_declaration_or_no_docs_at_all_is_a_change_of_the_text() {
        let base = text(&shelf(SHELF_DECL, SHELF_DOCS, ""), "struct", None);
        let docs = SHELF_DOCS.replace("A shelf of books.", "A shelf of old books.");
        let declaration = SHELF_DECL.replace("pub root", "pub base");
        for (what, html) in [
            ("docs", shelf(SHELF_DECL, &docs, "")),
            ("declaration", shelf(&declaration, SHELF_DOCS, "")),
            ("no docs", shelf(SHELF_DECL, "", "")),
        ] {
            assert_ne!(base, text(&html, "struct", None), "{what}");
        }
    }

    #[test]
    fn a_public_field_added_to_the_declaration_of_a_type_is_a_change_of_its_text() {
        // The declaration of a struct lists its public fields, as rustdoc writes
        // it: a field gained is part of what the type promises.
        let more = SHELF_DECL.replace(
            "    pub root: <a class=\"struct\" href=\"x.html\" title=\"struct alloc::string::String\">String</a>,\n",
            "    pub root: <a class=\"struct\" href=\"x.html\" title=\"struct alloc::string::String\">String</a>,\n    pub shelves: u8,\n",
        );
        assert_ne!(more, SHELF_DECL);
        let before = text(&shelf(SHELF_DECL, SHELF_DOCS, ""), "struct", None);
        let after = text(&shelf(&more, SHELF_DOCS, ""), "struct", None);
        assert_ne!(before, after);
        assert_ne!(lines_hash(&before), lines_hash(&after));
        assert!(after.contains("decl     pub shelves: u8,\n"), "{after}");
    }

    #[test]
    fn a_docblock_in_single_quotes_is_read() {
        let docs = SHELF_DOCS.replace("class=\"docblock\"", "class='docblock'");
        assert_eq!(
            text(&shelf(SHELF_DECL, &docs, ""), "struct", None),
            text(&shelf(SHELF_DECL, SHELF_DOCS, ""), "struct", None)
        );
        let decl = SHELF_DECL.replace("class=\"rust item-decl\"", "class='rust item-decl'");
        assert_eq!(
            text(&shelf(&decl, SHELF_DOCS, ""), "struct", None),
            text(&shelf(SHELF_DECL, SHELF_DOCS, ""), "struct", None)
        );
    }

    /// The methods of a type, as rustdoc lays them out: the first with docs in a
    /// toggle, the second without, the third deprecated with docs.
    const METHODS: &str = "<h2 id=\"implementations\" class=\"section-header\">Implementations<a href=\"#implementations\" class=\"anchor\">§</a></h2><div id=\"implementations-list\"><details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Shelf\" class=\"impl\"><a class=\"src rightside\" href=\"s.html#1\">Source</a><a href=\"#impl-Shelf\" class=\"anchor\">§</a><h3 class=\"code-header\">impl Shelf</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.put\" class=\"method\"><a class=\"src rightside\" href=\"s.html#2\">Source</a><h4 class=\"code-header\">pub fn <a href=\"#method.put\" class=\"fn\">put</a>(&amp;mut self, book: &amp;<a class=\"primitive\" href=\"s\">str</a>) -&gt; <a class=\"enum\" href=\"o\">Option</a>&lt;u8&gt;</h4></section></summary><div class=\"docblock\"><p>Puts a <code>book</code> on the shelf.</p>\n<p>It may fail.</p>\n</div></details><section id=\"method.count\" class=\"method\"><a class=\"src rightside\" href=\"s.html#3\">Source</a><h4 class=\"code-header\">pub fn <a href=\"#method.count\" class=\"fn\">count</a>&lt;T&gt;(&amp;self) -&gt; u8<div class=\"where\">where\n    T: Clone,</div></h4></section><details class=\"toggle method-toggle deprecated\" open><summary><section id=\"method.old\" class=\"method\"><a class=\"src rightside\" href=\"s.html#4\">Source</a><h4 class=\"code-header\">pub fn <a href=\"#method.old\" class=\"fn\">old</a>(&amp;self)</h4></section><span class=\"item-info\"><div class=\"stab deprecated\"><span class=\"emoji\">x</span><span>Deprecated: no</span></div></span></summary><div class=\"docblock\"><p>Gone soon.</p>\n</div></details></div></details></div>";

    #[test]
    fn a_member_promises_its_code_header_and_the_docblock_after_it() {
        let html = shelf(SHELF_DECL, SHELF_DOCS, METHODS);
        assert_eq!(
            text(&html, "method", Some("method.put")),
            "decl pub fn put(&mut self, book: &str) -> Option<u8>\ndoc Puts a book on the shelf.\n\ndoc It may fail.\n"
        );
        // No docs: only the header, which is what the member promises; the `where`
        // clause is on a line of its own, as rustdoc breaks it.
        assert_eq!(
            text(&html, "method", Some("method.count")),
            "decl pub fn count<T>(&self) -> u8\ndecl where\ndecl     T: Clone,\n"
        );
        // The mark of a deprecated item is not the docs.
        assert_eq!(
            text(&html, "method", Some("method.old")),
            "decl pub fn old(&self)\ndoc Gone soon.\n"
        );
    }

    #[test]
    fn an_impl_added_after_the_last_method_changes_nothing_of_it() {
        let before = shelf(SHELF_DECL, SHELF_DOCS, METHODS);
        let trait_impl = "<h2 id=\"trait-implementations\" class=\"section-header\">Trait Implementations<a href=\"#trait-implementations\" class=\"anchor\">§</a></h2><div id=\"trait-implementations-list\"><details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Clone-for-Shelf\" class=\"impl\"><h3 class=\"code-header\">impl Clone for Shelf</h3></section></summary><div class=\"docblock\"><p>Not a member's docs.</p>\n</div></details></div>";
        let after = shelf(SHELF_DECL, SHELF_DOCS, &format!("{METHODS}{trait_impl}"));
        for anchor in ["method.put", "method.count", "method.old"] {
            assert_eq!(
                text(&before, "method", Some(anchor)),
                text(&after, "method", Some(anchor)),
                "{anchor}"
            );
        }
        // The last method has no docs of its own, and the docs of what follows it
        // are not its docs.
        let last = METHODS.replace(
            "<details class=\"toggle method-toggle deprecated\" open><summary>",
            "<details class=\"toggle method-toggle\" open><summary>",
        );
        let bare = last
            .replace("<span class=\"item-info\"><div class=\"stab deprecated\"><span class=\"emoji\">x</span><span>Deprecated: no</span></div></span>", "")
            .replace("</summary><div class=\"docblock\"><p>Gone soon.</p>\n</div></details>", "</summary></details>");
        let with_impl = shelf(SHELF_DECL, SHELF_DOCS, &format!("{bare}{trait_impl}"));
        assert_eq!(
            text(&with_impl, "method", Some("method.old")),
            "decl pub fn old(&self)\n"
        );
    }

    #[test]
    fn a_field_promises_its_code_and_the_docblock_after_it() {
        let fields = "<h2 id=\"fields\" class=\"fields section-header\">Fields<a href=\"#fields\" class=\"anchor\">§</a></h2><span id=\"structfield.root\" class=\"structfield section-header\"><a href=\"#structfield.root\" class=\"anchor field\">§</a><code>root: <a class=\"struct\" href=\"x\">String</a></code></span><div class=\"docblock\"><p>Where it stands.</p>\n</div><span id=\"structfield.bare\" class=\"structfield section-header\"><a href=\"#structfield.bare\" class=\"anchor field\">§</a><code>bare: u8</code></span><h2 id=\"implementations\" class=\"section-header\">Implementations</h2><div class=\"docblock\"><p>Not the field's.</p>\n</div>";
        let html = shelf(SHELF_DECL, SHELF_DOCS, fields);
        assert_eq!(
            text(&html, "structfield", Some("structfield.root")),
            "decl root: String\ndoc Where it stands.\n"
        );
        // The field with no docs: the docblock after the next heading is not its own.
        assert_eq!(
            text(&html, "structfield", Some("structfield.bare")),
            "decl bare: u8\n"
        );
    }

    #[test]
    fn a_variant_promises_its_fields_too() {
        let variants = "<h2 id=\"variants\" class=\"variants section-header\">Variants<a href=\"#variants\" class=\"anchor\">§</a></h2><div class=\"variants\"><section id=\"variant.Gone\" class=\"variant\"><a href=\"#variant.Gone\" class=\"anchor\">§</a><h3 class=\"code-header\">Gone</h3></section><div class=\"docblock\"><p>It is gone.</p>\n</div><section id=\"variant.Moved\" class=\"variant\"><a href=\"#variant.Moved\" class=\"anchor\">§</a><h3 class=\"code-header\">Moved</h3></section><div class=\"docblock\"><p>It moved.</p>\n</div><div class=\"sub-variant\" id=\"variant.Moved.fields\"><h4>Fields</h4><div class=\"sub-variant-field\"><span id=\"variant.Moved.field.to\" class=\"section-header\"><a href=\"#variant.Moved.field.to\" class=\"anchor field\">§</a><code>to: String</code></span><div class=\"docblock\"><p>Where to.</p>\n</div></div></div><section id=\"variant.Plain\" class=\"variant\"><a href=\"#variant.Plain\" class=\"anchor\">§</a><h3 class=\"code-header\">Plain</h3></section><section id=\"variant.Bare\" class=\"variant\"><a href=\"#variant.Bare\" class=\"anchor\">§</a><h3 class=\"code-header\">Bare</h3></section><div class=\"sub-variant\" id=\"variant.Bare.fields\"><h4>Fields</h4><div class=\"sub-variant-field\"><span id=\"variant.Bare.field.n\" class=\"section-header\"><code>n: u8</code></span></div></div></div>";
        let html = page(&format!(
            "{}<pre class=\"rust item-decl\"><code>pub enum Cause {{ }}</code></pre>{variants}",
            heading("Enum", "Cause")
        ));
        assert_eq!(
            text(&html, "variant", Some("variant.Gone")),
            "decl Gone\ndoc It is gone.\n"
        );
        // The fields are part of the variant's promise: one added or dropped, or
        // the docs of one, is a change of the variant.
        assert_eq!(
            text(&html, "variant", Some("variant.Moved")),
            "decl Moved\ndoc It moved.\ndoc Fields\ndoc to: String\ndoc Where to.\n"
        );
        assert_eq!(
            text(&html, "variant", Some("variant.Plain")),
            "decl Plain\n"
        );
        assert_eq!(
            text(&html, "variant", Some("variant.Bare")),
            "decl Bare\ndoc Fields\ndoc n: u8\n"
        );
    }

    #[test]
    fn a_module_a_crate_a_primitive_and_a_keyword_promise_their_docs_only() {
        let docs = "<details class=\"toggle top-doc\" open><summary class=\"hideme\"><span>Expand description</span></summary><div class=\"docblock\"><p>The shelf.</p>\n<h2 id=\"the-rules\"><a class=\"doc-anchor\" href=\"#the-rules\">§</a>The rules</h2>\n<p>One rule.</p>\n</div></details><h2 id=\"structs\" class=\"section-header\">Structs<a href=\"#structs\" class=\"anchor\">§</a></h2><dl class=\"item-table\"><dt><a class=\"struct\" href=\"s\">Shelf</a></dt><dd>Not the module's docs.</dd></dl>";
        let html = page(&format!("{}{docs}", heading("Module", "shelf")));
        for kind in ["module", "primitive", "keyword"] {
            let promise = promise(&html, kind, None).unwrap();
            assert!(
                matches!(promise, Promise::DocsOnly(Docs::Written(_))),
                "{kind}"
            );
            // The `§` of the heading and the line `Source` are not in it.
            assert_eq!(
                promise.text(),
                "doc The shelf.\n\ndoc The rules\n\ndoc One rule.\n",
                "{kind}"
            );
        }
        // A module with no docs promises that.
        let bare = page(&format!(
            "{}<h2 id=\"structs\" class=\"section-header\">Structs</h2><dl class=\"item-table\"><dd>Not the module's docs.</dd></dl>",
            heading("Module", "shelf")
        ));
        assert_eq!(
            promise(&bare, "module", None).unwrap(),
            Promise::DocsOnly(Docs::Absent)
        );
        assert_eq!(text(&bare, "module", None), "");
    }

    #[test]
    fn a_kind_with_a_declaration_and_a_page_without_one_cannot_be_read() {
        let html = page(&format!("{}{SHELF_DOCS}", heading("Struct", "Shelf")));
        for kind in ["struct", "fn", "attr", "derive", "macro", "type", "trait"] {
            assert_eq!(
                promise(&html, kind, None),
                Err(Unreadable::NoDeclaration),
                "{kind}"
            );
        }
        // A member with no code header has no declaration either.
        let member = shelf(
            SHELF_DECL,
            SHELF_DOCS,
            "<section id=\"method.put\" class=\"method\"></section>",
        );
        assert_eq!(
            promise(&member, "method", Some("method.put")),
            Err(Unreadable::NoDeclaration)
        );
        // The kinds with none read the same page.
        assert!(promise(&html, "module", None).is_ok());
        // `attr` and `derive` are written with a declaration (`#[test]`), so they
        // are the kinds that have one.
        let attr = page(&format!(
            "{}<pre class=\"rust item-decl\"><code>#[marker]</code></pre>",
            heading("Attribute Macro", "marker")
        ));
        assert_eq!(text(&attr, "attr", None), "decl #[marker]\n");
    }

    #[test]
    fn a_layout_that_is_not_read_says_so() {
        assert_eq!(
            promise("<html><body>no main</body></html>", "struct", None),
            Err(Unreadable::Layout("the page has no main content"))
        );
        let html = shelf(SHELF_DECL, SHELF_DOCS, "");
        assert_eq!(
            promise(&html, "method", Some("method.nowhere")),
            Err(Unreadable::Layout("the member is not on its page"))
        );
        let open = page("<pre class=\"rust item-decl\"><code>pub struct S;");
        assert_eq!(promise(&open, "struct", None), Err(NOT_CLOSED));
    }

    #[test]
    fn entities_are_put_back_and_the_tags_of_rustdoc_leave_no_text() {
        let decl = "<pre class=\"rust item-decl\"><code>pub fn f(a: &amp;str) -&gt; <a href=\"x\" title=\"it&#39;s &quot;t&quot;\">T</a>&lt;&#x27;a, &#60;&#62;&gt; &amp;bogus; &#xZZ; &nbsp;<!-- no --></code></pre>";
        let html = page(&format!("{}{decl}", heading("Function", "f")));
        assert_eq!(
            text(&html, "fn", None),
            "decl pub fn f(a: &str) -> T<'a, <>> &bogus; &#xZZ; \u{a0}\n"
        );
        // A `>` in a quoted attribute does not end the tag.
        let quoted =
            "<pre class='rust item-decl'><code>x<a title='a>b' href=\"c>d\">y</a>z</code></pre>";
        let html = page(&format!("{}{quoted}", heading("Function", "f")));
        assert_eq!(text(&html, "fn", None), "decl xyz\n");
    }

    #[test]
    fn a_blank_line_added_to_the_docs_counts_for_bytes_and_not_for_lines() {
        let one = shelf(SHELF_DECL, SHELF_DOCS, "");
        let blank = SHELF_DOCS.replace("</p>\n<p>It", "</p>\n\n<p>It");
        let two = shelf(SHELF_DECL, &blank, "");
        let (one, two) = (text(&one, "struct", None), text(&two, "struct", None));
        assert_ne!(one, two);
        assert_eq!(lines_hash(&one), lines_hash(&two));
        assert_ne!(bytes_hash(&one), bytes_hash(&two));
        // Another word is another line for both.
        let word = SHELF_DOCS.replace("of books", "of old books");
        let three = text(&shelf(SHELF_DECL, &word, ""), "struct", None);
        assert_ne!(lines_hash(&one), lines_hash(&three));
    }

    #[test]
    fn a_line_of_the_docs_never_reads_as_a_line_of_the_declaration() {
        let docs = SHELF_DOCS.replace("A shelf of books.", "decl pub struct Shelf;");
        let text = text(&shelf(SHELF_DECL, &docs, ""), "struct", None);
        assert!(text.contains("\ndoc decl pub struct Shelf;\n"), "{text}");
        assert!(!text.contains("\ndecl pub struct Shelf;"), "{text}");
    }
}
