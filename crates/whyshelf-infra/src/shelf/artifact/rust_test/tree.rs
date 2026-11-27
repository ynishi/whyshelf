//! The crates of a project as trees of modules, for what a test holds: the modules of
//! each target and the file each is in, what each item compiles under, which targets a
//! file is in, how a module compiled only under test reaches tests, and what one test
//! holds. The rules are on the page of the space, "What a test holds"
//! ([`rust_test`](super)); this module is where they are carried out.
//!
//! | what | what it does |
//! |---|---|
//! | `ModuleTree::of` | the modules of one target, from its root file, by the rules of the reference for the file of a module; and what each item compiles under (`Compiled`) |
//! | `ModuleTrees::membership` | the targets a file is in, over the trees of all targets (`Membership`) |
//! | `resolve`, `Named::reaches` | what a path in a text names in a tree (`Named`), and whether it names a module |
//! | `ModuleTree::reach` | how a module compiled only under test reaches tests (`Reach`) |
//! | `ModuleTrees::held_by` | what a test holds (`Held`), or why it cannot be told |
//!
//! Each file is read once in a run, through the reading of the space (`source_at`,
//! within [`MAX_FILE_BYTES`](super::MAX_FILE_BYTES)), and lexed once for each way it is
//! compiled in the trees (always, or only under test), to [`MAX_DEPTH`](super::MAX_DEPTH);
//! a tree is held to [`MAX_TREE_FILES`](super::MAX_TREE_FILES) files and
//! [`MAX_TREE_BYTES`](super::MAX_TREE_BYTES) bytes. Nothing of the tokens is kept once
//! a file is scanned: what is kept is text, names and paths.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;

use proc_macro2::{Delimiter, Group, Literal, Spacing, TokenStream, TokenTree};
use sha2::{Digest, Sha256};
use whyshelf_core::app::shelf::Side;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonEmptyVec;

use super::super::targets::{Kind as TargetKind, Target, Targets, Unread};
use super::{
    Items, Kind, RustTestSpace, Scanned, Segment, Source, Spans, Text, Untellable, attribute_paths,
    beside, lexed, only_under_test, punct, without_docs,
};

/// The macros of the standard library that read a file by a path relative to the file
/// of the call.
const INCLUDES: &[&str] = &["include", "include_str", "include_bytes"];

/// What an item compiles under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Compiled {
    Always,
    /// Only under test: a `cfg` that holds only under test ([`only_under_test`]) is
    /// on it, outer or inner, or on a module around it, or it is of an integration
    /// test.
    OnlyUnderTest,
}

impl Compiled {
    /// What `attrs` say alone: only under test when one of them is a `cfg` that holds
    /// only under test.
    fn of<'a>(attrs: impl IntoIterator<Item = &'a Group>) -> Self {
        match attrs.into_iter().any(only_under_test) {
            true => Self::OnlyUnderTest,
            false => Self::Always,
        }
    }

    /// An item that says `inner` itself, inside what compiles as `self`.
    fn within(self, inner: Self) -> Self {
        match (self, inner) {
            (Self::Always, Self::Always) => Self::Always,
            _ => Self::OnlyUnderTest,
        }
    }
}

/// Whether a module stands under `#[macro_use]`, on its declaration or on that of a
/// module around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MacroUse {
    Not,
    Under,
}

impl MacroUse {
    /// What the attributes of one declaration say.
    fn of(attrs: &[Group]) -> Self {
        let under = attrs.iter().any(|attr| {
            attribute_paths(attr)
                .iter()
                .any(|path| path.len() == 1 && path[0] == "macro_use")
        });
        match under {
            true => Self::Under,
            false => Self::Not,
        }
    }

    fn within(self, inner: Self) -> Self {
        match (self, inner) {
            (Self::Not, Self::Not) => Self::Not,
            _ => Self::Under,
        }
    }
}

/// The `#[path]` of a module.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathAttr {
    Absent,
    Literal(String),
    /// One the scan cannot read as one plain literal, with why.
    Unread(String),
}

impl PathAttr {
    /// The `#[path]` among the outer attributes of a module.
    fn of(attrs: &[Group]) -> Self {
        for attr in attrs {
            let tokens: Vec<TokenTree> = attr.stream().into_iter().collect();
            if let [
                TokenTree::Ident(name),
                TokenTree::Punct(eq),
                TokenTree::Literal(file),
            ] = tokens.as_slice()
                && name == "path"
                && eq.as_char() == '='
            {
                return match literal_text(file) {
                    Some(text) => Self::Literal(text),
                    None => {
                        Self::Unread(format!("its `#[path = {file}]` is no plain string literal"))
                    }
                };
            }
            if attribute_paths(attr)
                .iter()
                .any(|path| path.len() == 1 && path[0] == "path")
            {
                return Self::Unread(
                    "its `#[path]` is not one literal the scan reads (under a `cfg_attr`, say)"
                        .to_owned(),
                );
            }
        }
        Self::Absent
    }
}

/// The text of a plain string literal (`"…"` with no escape, or a raw one); none for
/// any other literal.
fn literal_text(literal: &Literal) -> Option<String> {
    let text = literal.to_string();
    if let Some(rest) = text.strip_prefix('r') {
        let hashes = rest.len() - rest.trim_start_matches('#').len();
        let quoted = rest.get(hashes..rest.len().checked_sub(hashes)?)?;
        return quoted
            .strip_prefix('"')?
            .strip_suffix('"')
            .map(str::to_owned);
    }
    let inside = text.strip_prefix('"')?.strip_suffix('"')?;
    (!inside.contains('\\')).then(|| inside.to_owned())
}

/// A call of a macro that reads a file ([`INCLUDES`]): its path, when it is one
/// literal, or why it is not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum IncludeCall {
    Literal(String),
    Unread(String),
}

/// The macros whose arguments are expressions and patterns the scan reads as written:
/// the ones of the standard library, and the logging ones. A lone identifier in their
/// arguments is a local, a field or a method, not a module, so it is no path; in the
/// arguments of any other macro it may be built into one (`suite!(fixtures_tests)` may
/// expand to `$m::seed()`).
const PLAIN_MACROS: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "matches",
    "cfg",
    "format",
    "format_args",
    "write",
    "writeln",
    "print",
    "println",
    "eprint",
    "eprintln",
    "vec",
    "panic",
    "dbg",
    "todo",
    "unimplemented",
    "unreachable",
    "trace",
    "debug",
    "info",
    "warn",
    "error",
    "event",
    "span",
    "trace_span",
    "debug_span",
    "info_span",
    "warn_span",
    "error_span",
];

/// Words that can stand before a `!` that is no macro call (`if !(a && b)`).
const NOT_MACROS: &[&str] = &[
    "if", "while", "match", "return", "break", "in", "else", "let", "as",
];

/// Keywords beside [`NOT_MACROS`] after which a `::` starts a path of another crate
/// (`impl ::core::fmt::Debug for X`, `&mut ::x::Y`, `dyn ::x::Tr`).
const LEAD_WORDS: &[&str] = &["dyn", "impl", "mut", "for", "where", "move", "ref", "yield"];

/// Whether `word` is a keyword that ends no path: it is a run of its own, and a `::`
/// after it starts a path of another crate ([`NOT_MACROS`] and [`LEAD_WORDS`]). (Whether
/// a `!` after a word is no macro call is [`NOT_MACROS`] alone.)
fn ends_run(word: &str) -> bool {
    NOT_MACROS.contains(&word) || LEAD_WORDS.contains(&word)
}

/// Where a path starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Anchor {
    /// `crate::…` or `$crate::…`: the root of the target.
    Crate,
    /// `super::…`: this many parents up (`self::super::…` is the same as `super::…`).
    Super(usize),
    /// `self::…`: the module the path is written in.
    SelfMod,
    /// A first segment that names a module of that module, or a name brought in by a
    /// `use`, an item or the prelude (`Self` too).
    Plain,
    /// `::x::…`: another crate.
    Global,
}

/// Where a path stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Site {
    /// In code, or in the arguments of a macro of [`PLAIN_MACROS`].
    Code,
    /// In the body of a `macro_rules!` (but `$crate::…`): it is resolved where the macro
    /// is called, not where it is written.
    MacroBody,
    /// A lone identifier in the arguments of a macro not of [`PLAIN_MACROS`].
    LoneInMacro,
}

/// What a path is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Shape {
    /// A run of two or more segments in code.
    Path,
    /// One leaf of a `use` tree: its last segment can be a module.
    UseLeaf,
}

/// A path in some tokens, with where it is written: the inline modules between the
/// module of the list it is in and where it stands, how it starts, its segments after
/// the anchor, and its site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RefPath {
    inline: Vec<String>,
    anchor: Anchor,
    segments: Vec<String>,
    site: Site,
    shape: Shape,
}

impl RefPath {
    /// The path of `words` (the segments as written, unraw, `$crate` among them), led by
    /// `::` when `global`; none when there is none.
    fn of(
        global: bool,
        words: &[String],
        shape: Shape,
        site: Site,
        inline: &[String],
    ) -> Option<Self> {
        let first = words.first()?;
        let (anchor, from) = if global {
            (Anchor::Global, 0)
        } else if first == "crate" || first == "$crate" {
            (Anchor::Crate, 1)
        } else if first == "self" || first == "super" {
            let lead = words
                .iter()
                .take_while(|word| *word == "self" || *word == "super")
                .count();
            match words[..lead].iter().filter(|word| *word == "super").count() {
                0 => (Anchor::SelfMod, lead),
                up => (Anchor::Super(up), lead),
            }
        } else {
            (Anchor::Plain, 0)
        };
        Some(Self {
            inline: inline.to_vec(),
            anchor,
            segments: words[from..].to_vec(),
            site,
            shape,
        })
    }

    /// An identifier alone in the arguments of a macro.
    fn lone(word: String, inline: &[String]) -> Self {
        Self {
            inline: inline.to_vec(),
            anchor: Anchor::Plain,
            segments: vec![word],
            site: Site::LoneInMacro,
            shape: Shape::Path,
        }
    }
}

/// What some tokens name: each identifier in them (unraw, at any depth, those of a
/// macro call's tokens among them), each path in them (a run of two or more segments, or
/// a leaf of a `use` tree) with where it stands, and each call of a macro that reads a
/// file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Refs {
    names: BTreeSet<String>,
    /// The identifiers in member position (right after `.` or `::`), and the lone ones
    /// in the arguments of a macro not of [`PLAIN_MACROS`].
    members: BTreeSet<String>,
    paths: BTreeSet<RefPath>,
    includes: BTreeSet<IncludeCall>,
}

/// What the walk of some tokens is in.
#[derive(Debug, Clone, Copy)]
struct Within {
    /// The body of a `macro_rules!`.
    body: bool,
    /// The arguments of a macro that is not of [`PLAIN_MACROS`].
    lone: bool,
    /// The tokens of a `use`: only the identifiers in them count, its leaves are read
    /// apart.
    quiet: bool,
}

/// Whether `t[i]` and `t[i + 1]` are the two colons of `::`.
fn colons(t: &[TokenTree], i: usize) -> bool {
    matches!(t.get(i), Some(TokenTree::Punct(p)) if p.as_char() == ':' && p.spacing() == Spacing::Joint)
        && punct(t, i + 1, ':')
}

impl Refs {
    /// Adds what `tokens` name, written in the module of the list they are in.
    pub(super) fn add(&mut self, tokens: TokenStream) {
        self.add_in(tokens, &[]);
    }

    /// Adds what `tokens` name, written in the inline modules `inline` of the list they
    /// are in. Groups are walked by a stack, not by recursion.
    fn add_in(&mut self, tokens: TokenStream, inline: &[String]) {
        let start = Within {
            body: false,
            lone: false,
            quiet: false,
        };
        let mut stack: Vec<(Vec<TokenTree>, Within)> = vec![(tokens.into_iter().collect(), start)];
        while let Some((list, within)) = stack.pop() {
            let mut at = 0;
            while at < list.len() {
                at = self.step(&list, at, within, inline, &mut stack);
            }
        }
    }

    /// Reads the token at `list[at]`, and what goes with it; where the next one is.
    fn step(
        &mut self,
        list: &[TokenTree],
        at: usize,
        within: Within,
        inline: &[String],
        stack: &mut Vec<(Vec<TokenTree>, Within)>,
    ) -> usize {
        match &list[at] {
            TokenTree::Group(group) => {
                stack.push((group.stream().into_iter().collect(), within));
                at + 1
            }
            TokenTree::Literal(_) => at + 1,
            TokenTree::Punct(_) => {
                // A `::` that starts a path, not one after a type (`Vec::<u8>::new`):
                // first, after a punctuation but `>`, or after a keyword that ends no
                // path (`return ::x::y()`, `as ::x::Y`).
                let leads = at == 0
                    || match &list[at - 1] {
                        TokenTree::Punct(p) => p.as_char() != '>',
                        TokenTree::Ident(word) => ends_run(&word.to_string()),
                        TokenTree::Group(_) | TokenTree::Literal(_) => false,
                    };
                if !within.quiet
                    && colons(list, at)
                    && leads
                    && matches!(list.get(at + 2), Some(TokenTree::Ident(_)))
                {
                    return self.run(list, at + 2, true, within, inline, stack);
                }
                at + 1
            }
            TokenTree::Ident(ident) => {
                if within.quiet {
                    self.names.insert(Segment::of(&ident.to_string()).0);
                    return at + 1;
                }
                if ident == "use" {
                    let end = list[at + 1..]
                        .iter()
                        .position(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == ';'))
                        .map_or(list.len(), |p| at + 1 + p);
                    let site = match within.body {
                        true => Site::MacroBody,
                        false => Site::Code,
                    };
                    for (global, words) in use_leaves(&list[at + 1..end]) {
                        let site = match words.first().is_some_and(|word| word == "$crate") {
                            true => Site::Code,
                            false => site,
                        };
                        self.paths.extend(RefPath::of(
                            global,
                            &words,
                            Shape::UseLeaf,
                            site,
                            inline,
                        ));
                    }
                    self.names.insert("use".to_owned());
                    let quiet = Within {
                        quiet: true,
                        ..within
                    };
                    stack.push((list[at + 1..end].to_vec(), quiet));
                    return end;
                }
                self.run(list, at, false, within, inline, stack)
            }
        }
    }

    /// Reads the run of identifiers joined by `::` that starts at `list[at]` (after a
    /// leading `::` when `global`), and the macro call it is the name of, if it is one.
    fn run(
        &mut self,
        list: &[TokenTree],
        at: usize,
        global: bool,
        within: Within,
        inline: &[String],
        stack: &mut Vec<(Vec<TokenTree>, Within)>,
    ) -> usize {
        let dollar = at > 0 && punct(list, at - 1, '$');
        let mut words: Vec<String> = Vec::new();
        let mut k = at;
        while let Some(TokenTree::Ident(ident)) = list.get(k) {
            let word = Segment::of(&ident.to_string()).0;
            self.names.insert(word.clone());
            // After the first, a word follows `::`; the first follows `.` or `::` too
            // when the run starts there (`.m()`, `<X as Tr>::m()`).
            // One `.`, not the end of a range (`0..count`).
            let after_dot = k == at
                && at > 0
                && punct(list, at - 1, '.')
                && !(at > 1 && punct(list, at - 2, '.'));
            let after_colons = k == at && at >= 2 && colons(list, at - 2);
            if k > at || after_dot || after_colons {
                self.members.insert(word.clone());
            }
            words.push(match dollar && k == at && word == "crate" {
                true => "$crate".to_owned(),
                false => word,
            });
            k += 1;
            // A keyword that ends no path is a run of its own: the `::` after it starts
            // a path of another crate (`return ::x::y()`), read on its own.
            let alone = words.len() == 1 && ends_run(&words[0]);
            if !alone && colons(list, k) && matches!(list.get(k + 2), Some(TokenTree::Ident(_))) {
                k += 2;
            } else {
                break;
            }
        }
        let Some(last) = words.last() else {
            return at + 1;
        };
        if punct(list, k, '!') && !NOT_MACROS.contains(&last.as_str()) {
            // `macro_rules! name { … }`: its body is resolved where it is called.
            if words.len() == 1
                && last == "macro_rules"
                && let Some(TokenTree::Ident(name)) = list.get(k + 1)
                && let Some(TokenTree::Group(body)) = list.get(k + 2)
            {
                self.names.insert(Segment::of(&name.to_string()).0);
                let body_within = Within {
                    body: true,
                    ..within
                };
                stack.push((body.stream().into_iter().collect(), body_within));
                return k + 3;
            }
            if let Some(TokenTree::Group(args)) = list.get(k + 1) {
                if INCLUDES.contains(&last.as_str())
                    && let Some(call) = include_call(last, args)
                {
                    self.includes.insert(call);
                }
                let call_within = Within {
                    lone: !PLAIN_MACROS.contains(&last.as_str()),
                    ..within
                };
                stack.push((args.stream().into_iter().collect(), call_within));
                // A path to a macro names the module it is in.
                self.add_path(global, &words, dollar, within, inline);
                return k + 2;
            }
        }
        let ticked = at > 0 && punct(list, at - 1, '\'');
        if words.len() > 1 {
            self.add_path(global, &words, dollar, within, inline);
        } else if within.lone && !ticked && !dollar {
            let word = words.remove(0);
            self.members.insert(word.clone());
            self.paths.insert(RefPath::lone(word, inline));
        }
        k
    }

    /// Adds the path `words`, of two or more segments, written where `within` says.
    fn add_path(
        &mut self,
        global: bool,
        words: &[String],
        dollar: bool,
        within: Within,
        inline: &[String],
    ) {
        if words.len() < 2 {
            return;
        }
        let site = match (within.body, dollar && words[0] == "$crate") {
            (true, false) => Site::MacroBody,
            _ => Site::Code,
        };
        self.paths
            .extend(RefPath::of(global, words, Shape::Path, site, inline));
    }

    /// Adds what `other` names, which stands in the inline modules `steps` of the list
    /// this stands in, after its own.
    fn extend(&mut self, other: &Self, steps: &[String]) {
        self.names.extend(other.names.iter().cloned());
        self.members.extend(other.members.iter().cloned());
        self.includes.extend(other.includes.iter().cloned());
        self.paths.extend(other.paths.iter().map(|path| RefPath {
            inline: steps.iter().chain(&path.inline).cloned().collect(),
            ..path.clone()
        }));
    }
}

/// The leaves of the `use` tree `t` (what follows `use`, before the `;`), each as the
/// words of its path, and whether it is led by `::`: `a::{b, c::d as e}` gives `a::b`
/// and `a::c::d`; a glob `a::*` gives `a`; `a::{self}` gives `a`. Nested groups are
/// walked by a stack.
fn use_leaves(t: &[TokenTree]) -> Vec<(bool, Vec<String>)> {
    let mut leaves = Vec::new();
    let mut stack: Vec<(bool, Vec<String>, Vec<TokenTree>)> = vec![(false, Vec::new(), t.to_vec())];
    while let Some((led, prefix, t)) = stack.pop() {
        let mut k = 0;
        while k < t.len() {
            let mut words = prefix.clone();
            let mut global = led;
            let mut leaf = true;
            if prefix.is_empty() && colons(&t, k) {
                global = true;
                k += 2;
            }
            loop {
                match t.get(k) {
                    Some(TokenTree::Punct(p)) if p.as_char() == '$' => k += 1,
                    Some(TokenTree::Ident(ident)) => {
                        let word = Segment::of(&ident.to_string()).0;
                        words.push(match k > 0 && punct(&t, k - 1, '$') && word == "crate" {
                            true => "$crate".to_owned(),
                            false => word,
                        });
                        k += 1;
                        if colons(&t, k) {
                            k += 2;
                        } else {
                            break;
                        }
                    }
                    Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                        stack.push((global, words.clone(), group.stream().into_iter().collect()));
                        leaf = false;
                        k += 1;
                        break;
                    }
                    Some(TokenTree::Punct(p)) if p.as_char() == '*' => {
                        k += 1;
                        break;
                    }
                    _ => break,
                }
            }
            if leaf {
                if words.len() == prefix.len() + 1
                    && !prefix.is_empty()
                    && words.last().is_some_and(|word| word == "self")
                {
                    words.pop();
                }
                leaves.push((global, words));
            }
            while k < t.len() && !punct(&t, k, ',') {
                k += 1;
            }
            k += 1;
        }
    }
    leaves
}

/// The call `name!(args)` of a macro that reads a file: its literal path, or why it
/// has none; none when its tokens are a fragment of a macro's definition (`$path`).
fn include_call(name: &str, args: &Group) -> Option<IncludeCall> {
    let inside: Vec<TokenTree> = args.stream().into_iter().collect();
    if inside
        .iter()
        .any(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == '$'))
    {
        return None;
    }
    let literal = match inside.as_slice() {
        [TokenTree::Literal(path)] => literal_text(path),
        [TokenTree::Literal(path), TokenTree::Punct(comma)] if comma.as_char() == ',' => {
            literal_text(path)
        }
        _ => None,
    };
    Some(match literal {
        Some(path) => IncludeCall::Literal(path),
        None => IncludeCall::Unread(format!("`{name}!{args}` reads a path that is no literal")),
    })
}

/// What [`Refs`] the items beside in a test part name, read as they are printed
/// ([`beside`]): every item but the tests, an inline module read through (a path in it
/// keeps the inline modules between the part and where it stands), a module declared
/// out of line left out (a declaration is no use).
pub(super) fn refs_beside(items: &Items, refs: &mut Refs) {
    refs_beside_in(items, &mut Vec::new(), refs);
}

/// [`refs_beside`] for the items of the inline modules `inline` of the part.
fn refs_beside_in(items: &Items, inline: &mut Vec<String>, refs: &mut Refs) {
    for item in &items.items {
        match &item.kind {
            Kind::Fn(_) if item.is_test() => {}
            Kind::Mod {
                name,
                body: Some(body),
            } => {
                inline.push(Segment::of(&name.to_string()).0);
                refs_beside_in(&Items::scan(body.stream()), inline, refs);
                inline.pop();
            }
            Kind::Mod { body: None, .. } => {}
            _ => refs.add_in(without_docs(items.tokens_of(item)), inline),
        }
    }
}

/// What one test's text names, apart from the text: what its own text and the items
/// beside it name, and the inline modules of the file its test parts are (none: the
/// file), each part with its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Reached {
    pub(super) own: Refs,
    pub(super) parts: Vec<(Vec<String>, Arc<Refs>)>,
    /// The inline modules of its file the test is in, unraw.
    pub(super) module: Vec<String>,
}

/// The path of a module in its target, its segments unraw; none is the root.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ModulePath(Vec<String>);

impl ModulePath {
    fn joined(&self, more: &[String]) -> Self {
        Self(self.0.iter().chain(more).cloned().collect())
    }

    /// The module `steps` parents up; none when that is above the root.
    fn up(&self, steps: usize) -> Option<Self> {
        let keep = self.0.len().checked_sub(steps)?;
        Some(Self(self.0[..keep].to_vec()))
    }
}

impl fmt::Display for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("crate")?;
        for segment in &self.0 {
            write!(f, "::{segment}")?;
        }
        Ok(())
    }
}

/// One module a test holds, as the text writes it: its path in its target, its file,
/// and its held text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HeldModule {
    pub(super) module: ModulePath,
    pub(super) file: RelPath,
    pub(super) text: Arc<str>,
}

/// One file a test holds by an include: its path, and the SHA-256 of its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Include {
    pub(super) path: RelPath,
    pub(super) sha256: [u8; 32],
}

impl Include {
    /// The 64 lowercase hex characters of the digest.
    pub(super) fn hex(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(64);
        for byte in self.sha256 {
            out.push(char::from(DIGITS[usize::from(byte >> 4)]));
            out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        out
    }
}

/// What a test holds: the modules, in the order of `(file, module)`, the test residue
/// of each module of code among them, and the files its includes read, in the order
/// of their paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Held {
    pub(super) modules: Vec<HeldModule>,
    pub(super) includes: Vec<Include>,
}

impl Held {
    pub(super) fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.includes.is_empty()
    }

    /// The files a test of the file at `file` that holds this is read from: its own,
    /// each held module's, and each include's.
    pub(super) fn files(&self, file: &RelPath) -> BTreeSet<RelPath> {
        std::iter::once(file.clone())
            .chain(self.modules.iter().map(|m| m.file.clone()))
            .chain(self.includes.iter().map(|i| i.path.clone()))
            .collect()
    }
}

/// One file, scanned: each list of items in it (the file, then each inline module,
/// in the order of the file, a module before the modules in it), with what the trees
/// need of each.
struct FileScan {
    lists: Vec<List>,
}

/// One list of items of a file, scanned.
struct List {
    /// The inline modules on the way to it from the top of the file, unraw.
    inline: Vec<String>,
    /// What its items compile under, before their own attributes.
    compiled: Compiled,
    macro_use: MacroUse,
    /// How the directory of its modules declared out of line is reached.
    step: Step,
    decls: Vec<Decl>,
    /// What its items compiled only under test name (every item, when it is so), the
    /// tests and the modules left out.
    test_refs: Refs,
    /// When it compiles always: its items that are compiled only under test, the tests
    /// and the modules left out, and those of code with a derive only under test (its
    /// test residue), in the order of the file.
    residue: Vec<ResidueItem>,
    /// When it compiles always: the modules a public glob `use` of its residue brings
    /// the names of in (`pub use M::*`), by name.
    globbed: BTreeSet<String>,
    /// When it is compiled only under test and nothing around it in the file is: what
    /// it is as a module a test may hold.
    unit: Option<Unit>,
    facts: Facts,
}

/// How the directory a list's modules out of line are under is reached from the one
/// around it.
enum Step {
    /// The top of the file.
    Top,
    /// An inline module by its name.
    Name(String),
    /// An inline module by its `#[path]`.
    Path(String),
    /// An inline module whose `#[path]` cannot be read, with why.
    Unread(String),
}

/// A module declared out of line (`mod x;`).
struct Decl {
    name: String,
    path: PathAttr,
    compiled: Compiled,
    macro_use: MacroUse,
}

/// A module compiled only under test that nothing around it in its file is: its held
/// text (none when it holds only tests and modules of tests), what its items and those
/// of the modules in it in the file name, and why a test may reach it without its
/// name, when one may.
struct Unit {
    text: Option<Arc<str>>,
    refs: Refs,
    everywhere: Option<String>,
}

/// What the items of a list say of how they are reached: the first macro exported,
/// the self type of each `impl` (none when it cannot be read), and the types declared.
#[derive(Default)]
struct Facts {
    exports: Option<String>,
    impls: Vec<Option<String>>,
    declares: BTreeSet<String>,
}

impl Facts {
    /// What the item `item` of `items` says.
    fn read(&mut self, items: &Items, item: &Scanned) {
        let t: Vec<TokenTree> = items.tokens_of(item).into_iter().collect();
        let (k, _) = item_keyword(&t, 2 * item.attrs.len());
        if is_word(&t, k, "macro_rules") && punct(&t, k + 1, '!') {
            let exported = item.attrs.iter().any(|attr| {
                attribute_paths(attr)
                    .iter()
                    .any(|path| path.len() == 1 && path[0] == "macro_export")
            });
            if exported && self.exports.is_none() {
                let name = match t.get(k + 2) {
                    Some(TokenTree::Ident(name)) => name.to_string(),
                    _ => String::new(),
                };
                self.exports = Some(format!(
                    "it defines the macro `{name}!` with `#[macro_export]`, which is called without the module's name"
                ));
            }
            return;
        }
        if is_word(&t, k, "impl") {
            self.impls.push(self_type(&t[k + 1..]));
        } else if let Some(name) = declared_by(&t, k, TYPE_KEYWORDS) {
            self.declares.insert(name);
        }
    }
}

/// The qualifiers that stand before the keyword of an item.
const QUALIFIERS: &[&str] = &["unsafe", "default", "auto"];

/// Where the keyword of an item is among its tokens `t`, whose attributes are the first
/// `after` tokens: past `pub` (or `pub(…)`) and the [`QUALIFIERS`]; and whether it is
/// `pub`. The one place the start of an item is found.
fn item_keyword(t: &[TokenTree], after: usize) -> (usize, bool) {
    let mut k = after;
    let public = is_word(t, k, "pub");
    if public {
        k += 1;
        if matches!(t.get(k), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
        {
            k += 1;
        }
    }
    while QUALIFIERS.iter().any(|word| is_word(t, k, word)) {
        k += 1;
    }
    (k, public)
}

/// The keywords of the items that declare a name.
const DECLARING: &[&str] = &[
    "struct", "enum", "union", "type", "trait", "const", "static",
];

/// The keyword at `t[at]` when it is one that [`DECLARING`] lists, and the name the item
/// declares after it (past the `mut` of `static mut`); none when there is no name.
fn declared_name(t: &[TokenTree], at: usize) -> Option<(&'static str, String)> {
    let keyword = DECLARING.iter().find(|word| is_word(t, at, word))?;
    let named = match is_word(t, at + 1, "mut") {
        true => at + 2,
        false => at + 1,
    };
    match t.get(named) {
        Some(TokenTree::Ident(name)) => Some((keyword, Segment::of(&name.to_string()).0)),
        _ => None,
    }
}

/// The name `t[at]` declares when its keyword is one of `keywords`.
fn declared_by(t: &[TokenTree], at: usize, keywords: &[&str]) -> Option<String> {
    declared_name(t, at)
        .filter(|(keyword, _)| keywords.contains(keyword))
        .map(|(_, name)| name)
}

/// The declaring keywords whose item a test reaches by its name ([`Facts`]: the types).
const TYPE_KEYWORDS: &[&str] = &["struct", "enum", "union", "type", "trait"];
/// The declaring keywords of a residue item held by its name.
const RESIDUE_KEYWORDS: &[&str] = &["struct", "enum", "union", "type", "const", "static"];
/// The declaring keywords of an associated item.
const ASSOCIATED_KEYWORDS: &[&str] = &["const", "type", "static"];

/// Whether `t[i]` is the identifier `word`.
fn is_word(t: &[TokenTree], i: usize, word: &str) -> bool {
    matches!(t.get(i), Some(TokenTree::Ident(ident)) if ident == word)
}

/// The name of the self type of an `impl` whose tokens after `impl` are `t`: the last
/// segment of its path; none when it is no path the scan reads (a reference, a tuple,
/// `dyn`).
fn self_type(t: &[TokenTree]) -> Option<String> {
    let mut start = 0;
    if punct(t, 0, '<') {
        start = after_angles(t, 0)?;
    }
    let mut depth = 0usize;
    let mut head = start;
    let mut end = start;
    while end < t.len() {
        match &t[end] {
            TokenTree::Punct(p) if p.as_char() == '<' => depth += 1,
            TokenTree::Punct(p) if p.as_char() == '>' && !arrow(t, end) => {
                depth = depth.saturating_sub(1);
            }
            TokenTree::Ident(w) if depth == 0 && w == "for" && !punct(t, end + 1, '<') => {
                head = end + 1;
            }
            TokenTree::Ident(w) if depth == 0 && w == "where" => break,
            TokenTree::Group(g) if depth == 0 && g.delimiter() == Delimiter::Brace => break,
            _ => {}
        }
        end += 1;
    }
    let path = &t[head..end];
    let mut k = 0;
    if punct(path, 0, ':') && punct(path, 1, ':') {
        k = 2;
    }
    let mut last = None;
    while let Some(TokenTree::Ident(ident)) = path.get(k) {
        last = Some(Segment::of(&ident.to_string()).0);
        if punct(path, k + 1, ':') && punct(path, k + 2, ':') {
            k += 3;
        } else {
            break;
        }
    }
    last.filter(|name| !matches!(name.as_str(), "dyn" | "impl" | "mut"))
}

/// Where the tokens after the `<` at `t[at]` and its `>` start; none when it is not
/// closed. The `>` of `->` and `=>` closes none.
fn after_angles(t: &[TokenTree], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (k, token) in t.iter().enumerate().skip(at) {
        match token {
            TokenTree::Punct(p) if p.as_char() == '<' => depth += 1,
            TokenTree::Punct(p) if p.as_char() == '>' && !arrow(t, k) => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(k + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether the `>` at `t[k]` ends `->` or `=>`.
fn arrow(t: &[TokenTree], k: usize) -> bool {
    matches!(
        k.checked_sub(1).and_then(|before| t.get(before)),
        Some(TokenTree::Punct(q)) if matches!(q.as_char(), '-' | '=') && q.spacing() == Spacing::Joint
    )
}

/// How a test reaches an item of a test residue (R2b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResidueKind {
    /// By an identifier of one of its names.
    ByName,
    /// By every test of the target: it is used with no name a test writes.
    Everywhere,
    /// A `use`: by every test in the subtree of its module, and by one of its names
    /// (what a public one brings in).
    Use,
}

/// What reaches an item of a test residue held by name: which set of a test's
/// [`Refs`] its names are matched against.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Via {
    /// An identifier anywhere of one of these names.
    Names(BTreeSet<String>),
    /// An identifier in member position (or a lone one in the arguments of a macro) of
    /// one of these names: the associated items of an `impl` or a `trait`, which are
    /// reached only so.
    Members(BTreeSet<String>),
    /// A trait: its own name as an identifier, or one of its items as a member.
    NameOrMembers {
        name: BTreeSet<String>,
        members: BTreeSet<String>,
    },
}

impl Via {
    /// Whether the identifiers `names` and the member names `members` held reach it.
    fn reached(&self, names: &BTreeSet<String>, members: &BTreeSet<String>) -> bool {
        let any = |wanted: &BTreeSet<String>, set: &BTreeSet<String>| {
            wanted.iter().any(|name| set.contains(name))
        };
        match self {
            Self::Names(wanted) => any(wanted, names),
            Self::Members(wanted) => any(wanted, members),
            Self::NameOrMembers { name, members: m } => any(name, names) || any(m, members),
        }
    }
}

/// One item of a test residue: how it is reached, what reaches it by name, its line,
/// and what it names.
struct ResidueItem {
    kind: ResidueKind,
    via: Via,
    text: Arc<str>,
    refs: Refs,
}

/// Whether an `impl` is of a trait or inherent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImplOf {
    Trait,
    Inherent,
}

/// The item `item` of `items`, whose tokens without docs are `tokens`, as an item of a
/// test residue: by name a `fn`, `const`, `static`, `struct`, `enum`, `union`, `type`
/// or `macro_rules!` (its name), an inherent `impl` (the names of its associated items)
/// and a `trait` (its name and those of its items); a `use` by its module's subtree and,
/// when public, by what it brings in, a public glob adding its module to `globbed`;
/// every other one (a trait `impl`, an `impl` with no associated item, an item macro
/// call) by every test. None when it prints as nothing.
fn residue_item(
    items: &Items,
    item: &Scanned,
    tokens: TokenStream,
    globbed: &mut BTreeSet<String>,
) -> Option<ResidueItem> {
    let text = tokens.to_string();
    if text.is_empty() {
        return None;
    }
    let mut refs = Refs::default();
    refs.add(tokens);
    let t: Vec<TokenTree> = items.tokens_of(item).into_iter().collect();
    let (k, public) = item_keyword(&t, 2 * item.attrs.len());
    let named = |at: usize| -> BTreeSet<String> {
        match t.get(at) {
            Some(TokenTree::Ident(name)) => BTreeSet::from([Segment::of(&name.to_string()).0]),
            _ => BTreeSet::new(),
        }
    };
    let (kind, via) =
        match &item.kind {
            Kind::Macro(_) => (ResidueKind::Everywhere, Via::Names(BTreeSet::new())),
            Kind::Fn(name) => (
                ResidueKind::ByName,
                Via::Names(BTreeSet::from([Segment::of(&name.to_string()).0])),
            ),
            _ if is_word(&t, k, "use") => {
                let tree = &t[k + 1..];
                let mut scope = super::Scope::default();
                super::named_in_use(tree, &mut scope);
                match public {
                    true => {
                        glob_modules(tree, globbed);
                        (ResidueKind::Use, Via::Names(scope.names))
                    }
                    false => (ResidueKind::Use, Via::Names(BTreeSet::new())),
                }
            }
            _ if is_word(&t, k, "macro_rules") && punct(&t, k + 1, '!') => {
                (ResidueKind::ByName, Via::Names(named(k + 2)))
            }
            _ => {
                if is_word(&t, k, "impl") {
                    let (of, body) = impl_shape(&t[k + 1..]);
                    let names = body.map(associated).unwrap_or_default();
                    match (of, names.is_empty()) {
                        (ImplOf::Inherent, false) => (ResidueKind::ByName, Via::Members(names)),
                        (ImplOf::Trait, _) | (ImplOf::Inherent, true) => {
                            (ResidueKind::Everywhere, Via::Names(BTreeSet::new()))
                        }
                    }
                } else if is_word(&t, k, "trait") {
                    let mut members = BTreeSet::new();
                    if let Some(TokenTree::Group(body)) = t.iter().rev().find(
                        |t| matches!(t, TokenTree::Group(g) if g.delimiter() == Delimiter::Brace),
                    ) {
                        members.extend(associated(body));
                    }
                    (
                        ResidueKind::ByName,
                        Via::NameOrMembers {
                            name: declared_by(&t, k, &["trait"]).into_iter().collect(),
                            members,
                        },
                    )
                } else if let Some(name) = declared_by(&t, k, RESIDUE_KEYWORDS) {
                    (ResidueKind::ByName, Via::Names(BTreeSet::from([name])))
                } else {
                    (ResidueKind::Everywhere, Via::Names(BTreeSet::new()))
                }
            }
        };
    Some(ResidueItem {
        kind,
        via,
        text: Arc::from(text),
        refs,
    })
}

/// Whether an `impl` whose tokens after `impl` are `t` is of a trait (a `for` at its
/// top, outside the generics), and its body.
fn impl_shape(t: &[TokenTree]) -> (ImplOf, Option<&Group>) {
    let start = match punct(t, 0, '<') {
        true => after_angles(t, 0).unwrap_or(t.len()),
        false => 0,
    };
    let mut depth = 0usize;
    let mut of = ImplOf::Inherent;
    for (end, token) in t.iter().enumerate().skip(start) {
        match token {
            TokenTree::Punct(p) if p.as_char() == '<' => depth += 1,
            TokenTree::Punct(p) if p.as_char() == '>' && !arrow(t, end) => {
                depth = depth.saturating_sub(1);
            }
            TokenTree::Ident(w) if depth == 0 && w == "for" && !punct(t, end + 1, '<') => {
                of = ImplOf::Trait;
            }
            TokenTree::Group(g) if depth == 0 && g.delimiter() == Delimiter::Brace => {
                return (of, Some(g));
            }
            _ => {}
        }
    }
    (of, None)
}

/// The names of the associated items of the body of an `impl` or a `trait`: each
/// function, constant, static and type.
fn associated(body: &Group) -> BTreeSet<String> {
    let inside = Items::scan(body.stream());
    let mut names = BTreeSet::new();
    for item in &inside.items {
        if let Kind::Fn(name) = &item.kind {
            names.insert(Segment::of(&name.to_string()).0);
            continue;
        }
        let t: Vec<TokenTree> = inside.tokens_of(item).into_iter().collect();
        let (k, _) = item_keyword(&t, 2 * item.attrs.len());
        names.extend(declared_by(&t, k, ASSOCIATED_KEYWORDS));
    }
    names
}

/// The modules a `use` tree brings every name of in (`M::*`), by the last segment
/// before the `*`, onto `out`; `self`, `super` and `crate` are none.
fn glob_modules(t: &[TokenTree], out: &mut BTreeSet<String>) {
    for (at, token) in t.iter().enumerate() {
        match token {
            TokenTree::Group(group) => {
                let inside: Vec<TokenTree> = group.stream().into_iter().collect();
                glob_modules(&inside, out);
            }
            TokenTree::Punct(p) if p.as_char() == '*' => {
                if let Some(before) = at.checked_sub(3)
                    && punct(t, before + 1, ':')
                    && punct(t, before + 2, ':')
                    && let Some(TokenTree::Ident(module)) = t.get(before)
                {
                    let module = Segment::of(&module.to_string()).0;
                    if !matches!(module.as_str(), "self" | "super" | "crate") {
                        out.insert(module);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Whether an item of code carries a derive that holds only under test
/// (`#[cfg_attr(test, derive(…))]`): a trait impl compiled only under test.
fn derives_under_test(attrs: &[Group]) -> bool {
    attrs.iter().any(|attr| {
        let tokens: Vec<TokenTree> = attr.stream().into_iter().collect();
        let [TokenTree::Ident(name), TokenTree::Group(list)] = tokens.as_slice() else {
            return false;
        };
        if name != "cfg_attr" || list.delimiter() != Delimiter::Parenthesis {
            return false;
        }
        let inside: Vec<TokenTree> = list.stream().into_iter().collect();
        let parts = super::predicates(&inside);
        let Some((condition, rest)) = parts.split_first() else {
            return false;
        };
        super::holds_only_under_test(condition)
            && rest
                .iter()
                .any(|part| matches!(part.first(), Some(TokenTree::Ident(d)) if d == "derive"))
    })
}

/// The held text of a list of items as a module a test holds: its inner attributes,
/// then its items but the tests as the items beside in a test part are printed
/// ([`beside`]); none when no item is printed.
fn unit_text(items: &Items) -> Option<Arc<str>> {
    let mut lines = Vec::new();
    beside(items, 0, &[], &mut lines);
    let lines: Vec<String> = lines
        .into_iter()
        .map(|tokens| without_docs(tokens).to_string())
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let inner = items
        .inner
        .iter()
        .map(|(at, _)| {
            without_docs(items.tokens[*at..*at + 3].iter().cloned().collect()).to_string()
        })
        .filter(|line| !line.is_empty());
    Some(Arc::from(inner.chain(lines).collect::<Vec<_>>().join("\n")))
}

/// The scan of the source `text` of the file at `path`, compiled as `base` says (only
/// under test when it is declared so, or of an integration test).
fn scan_file(path: &RelPath, text: &str, base: Compiled) -> Result<FileScan, Untellable> {
    let _spans = Spans;
    let top = Items::scan(lexed(path, text)?);
    let compiled = base.within(Compiled::of(top.inner.iter().map(|(_, attr)| attr)));
    let mut lists = Vec::new();
    let around = Compiled::Always;
    scan_list(
        &top,
        Vec::new(),
        Step::Top,
        (compiled, around),
        MacroUse::Not,
        &mut lists,
    );
    Ok(FileScan { lists })
}

/// Scans the list `items` (at `inline` in its file, reached by `step`, compiled as
/// the first of `compiled`, inside a list compiled as the second, `macro_use` on the
/// way) and the inline modules in it, onto `lists`.
fn scan_list(
    items: &Items,
    inline: Vec<String>,
    step: Step,
    (compiled, around): (Compiled, Compiled),
    macro_use: MacroUse,
    lists: &mut Vec<List>,
) {
    let at = lists.len();
    lists.push(List {
        inline: inline.clone(),
        compiled,
        macro_use,
        step,
        decls: Vec::new(),
        test_refs: Refs::default(),
        residue: Vec::new(),
        globbed: BTreeSet::new(),
        unit: None,
        facts: Facts::default(),
    });
    let mut decls = Vec::new();
    let mut test_refs = Refs::default();
    let mut residue: Vec<ResidueItem> = Vec::new();
    let mut globbed = BTreeSet::new();
    let mut facts = Facts::default();
    for item in &items.items {
        match &item.kind {
            Kind::Fn(_) if item.is_test() => {}
            Kind::Mod {
                name,
                body: Some(body),
            } => {
                let inside = Items::scan(body.stream());
                let own = Compiled::of(
                    item.attrs
                        .iter()
                        .chain(inside.inner.iter().map(|(_, attr)| attr)),
                );
                let name = Segment::of(&name.to_string()).0;
                let step = match PathAttr::of(&item.attrs) {
                    PathAttr::Absent => Step::Name(name.clone()),
                    PathAttr::Literal(path) => Step::Path(path),
                    PathAttr::Unread(why) => Step::Unread(why),
                };
                let mut path = inline.clone();
                path.push(name);
                scan_list(
                    &inside,
                    path,
                    step,
                    (compiled.within(own), compiled),
                    macro_use.within(MacroUse::of(&item.attrs)),
                    lists,
                );
            }
            Kind::Mod { name, body: None } => decls.push(Decl {
                name: Segment::of(&name.to_string()).0,
                path: PathAttr::of(&item.attrs),
                compiled: compiled.within(Compiled::of(&item.attrs)),
                macro_use: MacroUse::of(&item.attrs),
            }),
            _ => {
                let own = Compiled::of(&item.attrs);
                let tokens = without_docs(items.tokens_of(item));
                if compiled.within(own) == Compiled::OnlyUnderTest {
                    test_refs.add(tokens.clone());
                    facts.read(items, item);
                }
                if compiled == Compiled::Always {
                    match (own, derives_under_test(&item.attrs)) {
                        (Compiled::OnlyUnderTest, _) => {
                            residue.extend(residue_item(items, item, tokens, &mut globbed))
                        }
                        // An item of code whose derive holds only under test: a trait
                        // impl no name reaches.
                        (Compiled::Always, true) => {
                            residue.extend(
                                residue_item(items, item, tokens, &mut BTreeSet::new()).map(
                                    |item| ResidueItem {
                                        kind: ResidueKind::Everywhere,
                                        via: Via::Names(BTreeSet::new()),
                                        ..item
                                    },
                                ),
                            );
                        }
                        (Compiled::Always, false) => {}
                    }
                }
            }
        }
    }
    let unit = (compiled == Compiled::OnlyUnderTest && around == Compiled::Always).then(|| {
        // This list and those after it inside it, each pushed after it until one is not.
        let end = lists[at + 1..]
            .iter()
            .position(|list| !list.inline.starts_with(&inline))
            .map_or(lists.len(), |p| at + 1 + p);
        let mut refs = test_refs.clone();
        let mut exports = facts.exports.clone();
        let mut impls = facts.impls.clone();
        let mut declares = facts.declares.clone();
        for list in &lists[at + 1..end] {
            refs.extend(&list.test_refs, &list.inline[inline.len()..]);
            exports = exports.or_else(|| list.facts.exports.clone());
            impls.extend(list.facts.impls.iter().cloned());
            declares.extend(list.facts.declares.iter().cloned());
        }
        let foreign = impls.into_iter().find_map(|of| match of {
            Some(name) if declares.contains(&name) => None,
            Some(name) => Some(format!(
                "it holds an `impl` of `{name}`, a type it does not declare, whose items are called without the module's name"
            )),
            None => Some(
                "it holds an `impl` of a type the scan cannot read, whose items may be called without the module's name"
                    .to_owned(),
            ),
        });
        Unit {
            text: unit_text(items),
            refs,
            everywhere: exports.or(foreign),
        }
    });
    let list = &mut lists[at];
    list.decls = decls;
    list.test_refs = test_refs;
    list.residue = residue;
    list.globbed = globbed;
    list.unit = unit;
    list.facts = facts;
}

/// The bounds a tree is read within: [`MAX_TREE_FILES`](super::MAX_TREE_FILES) and
/// [`MAX_TREE_BYTES`](super::MAX_TREE_BYTES), lower in a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TreeBounds {
    pub(super) files: usize,
    pub(super) bytes: u64,
}

impl Default for TreeBounds {
    fn default() -> Self {
        Self {
            files: super::MAX_TREE_FILES,
            bytes: super::MAX_TREE_BYTES,
        }
    }
}

/// A file read for the trees.
#[derive(Clone)]
enum Read {
    Text(Arc<str>),
    NoFile,
    Untold(Untellable),
}

/// The files the trees of one run read: each file's text once, and its scan once for
/// each way it is compiled.
#[derive(Default)]
struct Reads {
    texts: HashMap<RelPath, Read>,
    scans: HashMap<(RelPath, Compiled), Result<Arc<FileScan>, Untellable>>,
}

impl Reads {
    async fn text(&mut self, space: &RustTestSpace, side: Side, path: &RelPath) -> Read {
        if let Some(read) = self.texts.get(path) {
            return read.clone();
        }
        let read = match space.source_at(side, path, space.lister.max_file()).await {
            Source::Bytes(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Read::Text(Arc::from(text)),
                Err(_) => Read::Untold(Untellable::NotUtf8(path.clone())),
            },
            Source::NoFile => Read::NoFile,
            Source::Untold(why) => Read::Untold(why),
        };
        self.texts.insert(path.clone(), read.clone());
        read
    }

    async fn scan(
        &mut self,
        path: &RelPath,
        text: Arc<str>,
        base: Compiled,
    ) -> Result<Arc<FileScan>, Untellable> {
        let key = (path.clone(), base);
        if let Some(scan) = self.scans.get(&key) {
            return scan.clone();
        }
        let at = path.clone();
        // The file may be as large as the bound: it is lexed on a blocking thread.
        let scanned = match tokio::task::spawn_blocking(move || scan_file(&at, &text, base)).await {
            Ok(scan) => scan.map(Arc::new),
            Err(e) => Err(Untellable::Stopped {
                path: path.clone(),
                why: e.to_string(),
            }),
        };
        self.scans.insert(key, scanned.clone());
        scanned
    }
}

/// A module of a tree: its path in the target, its file, and the list of items it is
/// in that file.
pub(super) struct Module {
    path: ModulePath,
    file: RelPath,
    scan: Arc<FileScan>,
    list: usize,
    macro_use: MacroUse,
}

impl Module {
    fn list(&self) -> Option<&List> {
        self.scan.lists.get(self.list)
    }
}

/// A module out of line of a tree whose file cannot be read: where it is declared, its
/// path (that of the module it is declared in, and its name), whether it stands under
/// `#[macro_use]`, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Unresolved {
    at: RelPath,
    path: ModulePath,
    under: MacroUse,
    why: String,
}

impl Unresolved {
    /// Its name; `crate` for the root of a tree.
    fn module(&self) -> &str {
        self.path.0.last().map_or("crate", String::as_str)
    }
}

/// What a path names in a tree, by [`resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Named<'a> {
    /// The module the path got to by its segments that name modules (none when its first
    /// segment names none), and the segments it names after that, which a `use`, an
    /// alias or a glob may have brought from a module: each but the last for a path in
    /// code, every one for a leaf of a `use`.
    At {
        at: Option<ModulePath>,
        heads: Vec<String>,
    },
    /// A module declared out of line whose file cannot be read.
    Unread(&'a Unresolved),
    /// Another crate, or above the root: no module of the tree.
    Outside,
}

impl Named<'_> {
    /// Whether the path names the module at `path` named `name`: it got to that module
    /// or one inside it, or one of its heads is `name`; a module that cannot be read, it
    /// is the one the path got to.
    fn reaches(&self, path: &ModulePath, name: &str) -> bool {
        match self {
            Self::At { at, heads } => {
                at.as_ref().is_some_and(|at| at.0.starts_with(&path.0))
                    || heads.iter().any(|head| head == name)
            }
            Self::Unread(unread) => unread.path == *path,
            Self::Outside => false,
        }
    }
}

/// What `path`, written in the module at `base` of `tree`, names: the anchor says where
/// it starts (`crate` the root, `super` parents up, `self` and a first segment that is a
/// module of `base` from `base`; another crate and above the root name nothing), and
/// its segments are walked while they name modules of the tree. The body of a
/// `macro_rules!` and a lone identifier in the arguments of a macro are resolved where
/// the macro is called, so every segment of them is kept as a head.
pub(super) fn resolve<'a>(tree: &'a ModuleTree, base: &ModulePath, path: &RefPath) -> Named<'a> {
    match path.site {
        Site::MacroBody | Site::LoneInMacro => {
            return Named::At {
                at: None,
                heads: path.segments.clone(),
            };
        }
        Site::Code => {}
    }
    let base = base.joined(&path.inline);
    let (start, anchored) = match path.anchor {
        Anchor::Global => return Named::Outside,
        Anchor::Crate => (ModulePath::default(), true),
        Anchor::Super(up) => match base.up(up) {
            Some(parent) => (parent, true),
            None => return Named::Outside,
        },
        Anchor::SelfMod => (base, true),
        Anchor::Plain => (base, false),
    };
    let mut at = start;
    let mut walked = 0;
    while let Some(segment) = path.segments.get(walked) {
        let deeper = at.joined(std::slice::from_ref(segment));
        if tree.paths.contains(&deeper) {
            at = deeper;
            walked += 1;
            continue;
        }
        if let Some(unread) = tree.unresolved.iter().find(|u| u.path == deeper) {
            return Named::Unread(unread);
        }
        break;
    }
    let rest = &path.segments[walked..];
    let heads = match path.shape {
        Shape::UseLeaf => rest.to_vec(),
        Shape::Path => rest[..rest.len().saturating_sub(1)].to_vec(),
    };
    Named::At {
        at: (anchored || walked > 0).then_some(at),
        heads,
    }
}

/// Whether a tree was read whole, or stopped at its bounds.
enum TreeRead {
    Whole,
    Cut(Unresolved),
}

/// How a module compiled only under test reaches tests: by a path that names it
/// ([`Named::reaches`], with its name), or, for a reason, every test of its target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Reach {
    ByName(String),
    Everywhere(String),
}

/// A module of a target still to read: its file, the stem of a file that is no
/// `mod.rs` (none for the root, a `mod.rs` and a file read by `#[path]`), what it
/// compiles under and whether it stands under `#[macro_use]`, its path, and where it is
/// declared.
struct Visit {
    file: RelPath,
    relative: Option<String>,
    base: Compiled,
    macro_use: MacroUse,
    path: ModulePath,
    declared: (RelPath, String),
}

/// The directories of a list: the one its `mod x;` are under, and the one its
/// `#[path]` is read from.
#[derive(Clone)]
struct Dirs {
    decl: Vec<String>,
    path: Vec<String>,
}

/// The modules of one target.
pub(super) struct ModuleTree {
    target: Target,
    modules: Vec<Module>,
    /// The path of each module, to tell a module from a name that is none.
    paths: BTreeSet<ModulePath>,
    unresolved: Vec<Unresolved>,
    read: TreeRead,
    /// The modules a public glob `use` of a test residue brings in, by name.
    globbed: BTreeSet<String>,
}

impl ModuleTree {
    /// The modules of `target` at `side`, from its root file, the file of each module
    /// declared out of line found by the rules of the reference: `x.rs` or `x/mod.rs`
    /// beside a root or a `mod.rs`, under the directory of the stem of any other
    /// file, each inline module a directory step (by its `#[path]`, when it has one);
    /// a `#[path]` read from the directory of the file, or, inside an inline module,
    /// from the directory of that module. A module whose file cannot be read is kept
    /// unresolved, with why; past the bounds, the tree is cut.
    async fn of(space: &RustTestSpace, side: Side, target: &Target, reads: &mut Reads) -> Self {
        let mut tree = Self {
            target: target.clone(),
            modules: Vec::new(),
            paths: BTreeSet::new(),
            unresolved: Vec::new(),
            read: TreeRead::Whole,
            globbed: BTreeSet::new(),
        };
        let base = match target.kind {
            TargetKind::Test => Compiled::OnlyUnderTest,
            TargetKind::Lib | TargetKind::Bin | TargetKind::Bench | TargetKind::Example => {
                Compiled::Always
            }
        };
        let mut stack = vec![Visit {
            file: target.root.clone(),
            relative: None,
            base,
            macro_use: MacroUse::Not,
            path: ModulePath::default(),
            declared: (target.root.clone(), "crate".to_owned()),
        }];
        let mut seen: BTreeSet<(RelPath, ModulePath)> = BTreeSet::new();
        let (mut files, mut bytes) = (0usize, 0u64);
        while let Some(visit) = stack.pop() {
            if !seen.insert((visit.file.clone(), visit.path.clone())) {
                continue;
            }
            let unresolved = |why: String| Unresolved {
                at: visit.declared.0.clone(),
                path: visit.path.clone(),
                under: visit.macro_use,
                why,
            };
            let text = match reads.text(space, side, &visit.file).await {
                Read::Text(text) => text,
                Read::NoFile => {
                    tree.unresolved
                        .push(unresolved(format!("{} is not there", visit.file)));
                    continue;
                }
                Read::Untold(why) => {
                    tree.unresolved.push(unresolved(why.to_string()));
                    continue;
                }
            };
            files += 1;
            bytes = bytes.saturating_add(u64::try_from(text.len()).unwrap_or(u64::MAX));
            if files > space.tree.files || bytes > space.tree.bytes {
                tree.read = TreeRead::Cut(Unresolved {
                    at: target.root.clone(),
                    path: ModulePath::default(),
                    under: MacroUse::Not,
                    why: format!(
                        "the crate `{}` of the package at {} ({}) has more than {} module files, or more than {} bytes of them, and its modules are not read further",
                        target.name,
                        super::super::targets::shown(&target.package),
                        target.root,
                        space.tree.files,
                        space.tree.bytes
                    ),
                });
                break;
            }
            let scan = match reads.scan(&visit.file, text, visit.base).await {
                Ok(scan) => scan,
                Err(why) => {
                    tree.unresolved.push(unresolved(why.to_string()));
                    continue;
                }
            };
            let mut file_dir: Vec<String> =
                visit.file.as_str().split('/').map(str::to_owned).collect();
            file_dir.pop();
            let dirs = dirs_of(&scan, &file_dir, visit.relative.as_deref());
            for (index, list) in scan.lists.iter().enumerate() {
                let path = visit.path.joined(&list.inline);
                let macro_use = visit.macro_use.within(list.macro_use);
                for decl in &list.decls {
                    let declared = (visit.file.clone(), decl.name.clone());
                    let under = macro_use.within(decl.macro_use);
                    let missing = |why: String| Unresolved {
                        at: visit.file.clone(),
                        path: path.joined(std::slice::from_ref(&decl.name)),
                        under,
                        why,
                    };
                    let dir = match dirs.get(index) {
                        Some(Ok(dir)) => dir,
                        Some(Err(why)) => {
                            tree.unresolved.push(missing(why.clone()));
                            continue;
                        }
                        None => continue,
                    };
                    let found = match &decl.path {
                        PathAttr::Literal(text) => match joined(&dir.path, text) {
                            Ok(file) => match reads.text(space, side, &file).await {
                                Read::Text(_) => Ok((file, None)),
                                Read::NoFile => {
                                    Err(format!("{file}, which its `#[path]` names, is not there"))
                                }
                                Read::Untold(why) => Err(why.to_string()),
                            },
                            Err(why) => Err(format!("its `#[path = \"{text}\"]` {why}")),
                        },
                        PathAttr::Absent => {
                            beside_or_under(space, side, reads, &dir.decl, &decl.name).await
                        }
                        PathAttr::Unread(why) => Err(why.clone()),
                    };
                    match found {
                        Ok((file, relative)) => stack.push(Visit {
                            file,
                            relative,
                            base: list.compiled.within(decl.compiled),
                            macro_use: under,
                            path: path.joined(std::slice::from_ref(&decl.name)),
                            declared,
                        }),
                        Err(why) => tree.unresolved.push(missing(why)),
                    }
                }
                if list.compiled == Compiled::Always {
                    tree.globbed.extend(list.globbed.iter().cloned());
                }
                tree.paths.insert(path.clone());
                tree.modules.push(Module {
                    path,
                    file: visit.file.clone(),
                    scan: scan.clone(),
                    list: index,
                    macro_use,
                });
            }
        }
        tree
    }

    /// How the module `module` of this tree, compiled only under test, reaches tests:
    /// every test of the target when a name of it can be used without its own (it is
    /// the root of the crate, it stands under `#[macro_use]`, a public glob `use` of the
    /// test residue brings its names in, it exports a macro, it holds an `impl` of a
    /// type it does not declare); else by its name.
    pub(super) fn reach(&self, module: &Module) -> Reach {
        let Some(last) = module.path.0.last() else {
            return Reach::Everywhere(format!(
                "it is the root of the crate `{}`, which its modules reach as `crate` and `super`",
                self.target.name
            ));
        };
        if module.macro_use == MacroUse::Under {
            return Reach::Everywhere(
                "it stands under `#[macro_use]`, so a macro of it is called without its name"
                    .to_owned(),
            );
        }
        if self.globbed.contains(last) {
            return Reach::Everywhere(format!(
                "a public glob `use {last}::*` of the test residue brings its names in without its own"
            ));
        }
        match module
            .list()
            .and_then(|list| list.unit.as_ref())
            .and_then(|unit| unit.everywhere.clone())
        {
            Some(why) => Reach::Everywhere(why),
            None => Reach::ByName(last.clone()),
        }
    }
}

/// The file of `mod <name>;` under the directory `dir`: `<name>.rs` (a file that is no
/// `mod.rs`, of the stem `<name>`), else `<name>/mod.rs`; or why neither can be read.
async fn beside_or_under(
    space: &RustTestSpace,
    side: Side,
    reads: &mut Reads,
    dir: &[String],
    name: &str,
) -> Result<(RelPath, Option<String>), String> {
    let one = joined(dir, &format!("{name}.rs"))?;
    match reads.text(space, side, &one).await {
        Read::Text(_) => return Ok((one, Some(name.to_owned()))),
        Read::Untold(why) => return Err(why.to_string()),
        Read::NoFile => {}
    }
    let two = joined(dir, &format!("{name}/mod.rs"))?;
    match reads.text(space, side, &two).await {
        Read::Text(_) => Ok((two, None)),
        Read::Untold(why) => Err(why.to_string()),
        Read::NoFile => Err(format!("neither {one} nor {two} is there")),
    }
}

/// The directories of each list of `scan`, a file under `file_dir` whose stem is
/// `relative` when it is no `mod.rs`: at the top, `mod x;` is under the directory of the
/// stem and `#[path]` is read from the file's; in an inline module, both are its own
/// directory, that of the list around it and its name, or its `#[path]` read as a
/// `#[path]` of the list around it is.
fn dirs_of(
    scan: &FileScan,
    file_dir: &[String],
    relative: Option<&str>,
) -> Vec<Result<Dirs, String>> {
    let mut dirs: Vec<Result<Dirs, String>> = Vec::with_capacity(scan.lists.len());
    for list in &scan.lists {
        let parent = list.inline.len().checked_sub(1).and_then(|depth| {
            dirs.iter()
                .zip(&scan.lists)
                .rev()
                .find(|(_, other)| other.inline.as_slice() == &list.inline[..depth])
                .map(|(dir, _)| dir.clone())
        });
        let dir = match (&list.step, parent) {
            (Step::Top, _) => {
                let mut decl = file_dir.to_vec();
                decl.extend(relative.map(str::to_owned));
                Ok(Dirs {
                    decl,
                    path: file_dir.to_vec(),
                })
            }
            (_, Some(Err(why))) => Err(why),
            (_, None) => Err("the module around it is not found".to_owned()),
            (Step::Name(name), Some(Ok(around))) => {
                let mut dir = around.decl;
                dir.push(name.clone());
                Ok(Dirs {
                    decl: dir.clone(),
                    path: dir,
                })
            }
            (Step::Path(text), Some(Ok(around))) => match joined_dir(&around.path, text) {
                Ok(dir) => Ok(Dirs {
                    decl: dir.clone(),
                    path: dir,
                }),
                Err(why) => Err(format!("its `#[path = \"{text}\"]` {why}")),
            },
            (Step::Unread(why), Some(Ok(_))) => Err(why.clone()),
        };
        dirs.push(dir);
    }
    dirs
}

/// The parts of the directory `dir` with the path `text` under it, `.` and `..` read;
/// or why it is none (absolute, or out of the root).
fn joined_dir(dir: &[String], text: &str) -> Result<Vec<String>, String> {
    if text.starts_with('/') {
        return Err("is absolute".to_owned());
    }
    let mut at = dir.to_vec();
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if at.pop().is_none() {
                    return Err("leads out of the root of the project".to_owned());
                }
            }
            part => at.push(part.to_owned()),
        }
    }
    Ok(at)
}

/// The file at the path `text` under the directory `dir` ([`joined_dir`]), or why it is
/// no path of the project.
fn joined(dir: &[String], text: &str) -> Result<RelPath, String> {
    let parts = joined_dir(dir, text)?;
    parts
        .join("/")
        .parse()
        .map_err(|e| format!("is no path of the project: {e}"))
}

/// The targets a file is in.
pub(super) enum Membership {
    One(At),
    Many(NonEmptyVec<At>),
    Unreached(Unreached),
}

/// A target, by its place among the trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct At(usize);

/// Why a file is in no target read.
pub(super) enum Unreached {
    /// Everything of the project was read, and no tree holds it.
    NoTarget,
    /// No tree holds it, and something of the project could not be read: it may be of
    /// a target not read.
    ProjectUnread(NonEmptyVec<Unread>),
    /// A tree that holds it, or one that may, was cut at its bounds.
    TreeUnread(Unresolved),
    /// No tree holds it, and a module of a tree could not be read: the file may be
    /// one of its modules.
    ModuleUnread(Unresolved),
}

/// The trees of every target of the project at a side.
pub(super) struct ModuleTrees {
    targets: Targets,
    trees: Vec<ModuleTree>,
    /// The digest of each file an include reads, or why it cannot be told, once in a
    /// reading.
    digests: std::sync::Mutex<HashMap<RelPath, Result<[u8; 32], String>>>,
}

impl ModuleTrees {
    /// The targets of the project at `side` and the tree of each.
    pub(super) async fn of(space: &RustTestSpace, side: Side) -> Self {
        let targets = Targets::of(&space.state, side).await;
        let mut reads = Reads::default();
        let mut trees = Vec::with_capacity(targets.all().len());
        for target in targets.all() {
            trees.push(ModuleTree::of(space, side, target, &mut reads).await);
        }
        Self {
            targets,
            trees,
            digests: std::sync::Mutex::default(),
        }
    }

    /// The targets the file at `file` is in, over the trees of all targets: one, many,
    /// or none, and then why (no target holds it and all was read; something of the
    /// project was not read; a tree was cut).
    pub(super) fn membership(&self, file: &RelPath) -> Membership {
        let mut ins = Vec::new();
        for (at, tree) in self.trees.iter().enumerate() {
            if tree.modules.iter().any(|module| module.file == *file) {
                if let TreeRead::Cut(why) = &tree.read {
                    return Membership::Unreached(Unreached::TreeUnread(why.clone()));
                }
                ins.push(At(at));
            }
        }
        if let (1, Some(one)) = (ins.len(), ins.first()) {
            return Membership::One(*one);
        }
        if let Ok(many) = NonEmptyVec::from_vec(ins) {
            return Membership::Many(many);
        }
        if let Some(why) = self.trees.iter().find_map(|tree| match &tree.read {
            TreeRead::Cut(why) => Some(why.clone()),
            TreeRead::Whole => None,
        }) {
            return Membership::Unreached(Unreached::TreeUnread(why));
        }
        if let Ok(unread) = NonEmptyVec::from_vec(self.targets.unread().to_vec()) {
            return Membership::Unreached(Unreached::ProjectUnread(unread));
        }
        // A module whose file cannot be read may be the one the file is under.
        match self
            .trees
            .iter()
            .find_map(|tree| tree.unresolved.first().cloned())
        {
            Some(missing) => Membership::Unreached(Unreached::ModuleUnread(missing)),
            None => Membership::Unreached(Unreached::NoTarget),
        }
    }

    /// What the test whose text is `text`, in the file at `file`, holds, by the rules
    /// R1 to R5 on the page of the space: the modules compiled only under test it
    /// reaches by a path (each path of the text, and of what is held, resolved from the
    /// module it is written in), closed over what they name, and those every test of
    /// its targets reaches; the test residue of each module of code of its targets; and
    /// the files its includes and theirs read. A file in no target holds nothing. What
    /// it cannot tell is [`Untellable::Helper`], with the files told so far.
    pub(super) async fn held_by(
        &self,
        space: &RustTestSpace,
        side: Side,
        text: &Text,
        file: &RelPath,
    ) -> Result<Held, Untellable> {
        let ats = match self.membership(file) {
            Membership::One(at) => vec![at],
            Membership::Many(ats) => ats.into_vec(),
            Membership::Unreached(Unreached::NoTarget) => return Ok(Held::default()),
            Membership::Unreached(Unreached::ProjectUnread(unread)) => {
                return Err(Untellable::Helper {
                    at: file.clone(),
                    why: format!(
                        "it is in no crate read, and some of the project could not be read, so it may be in one not read: {}",
                        unread
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("; ")
                    ),
                    told: vec![file.clone()],
                });
            }
            Membership::Unreached(Unreached::TreeUnread(why)) => {
                return Err(Untellable::Helper {
                    at: why.at,
                    why: why.why,
                    told: vec![file.clone()],
                });
            }
            Membership::Unreached(Unreached::ModuleUnread(missing)) => {
                return Err(Untellable::Helper {
                    at: file.clone(),
                    why: format!(
                        "it is in no crate read, and the module `{}` declared at {} cannot be read, so it may be under it: {}",
                        missing.module(),
                        missing.at,
                        missing.why
                    ),
                    told: vec![file.clone()],
                });
            }
        };
        let reached = &text.reached;
        let parts: Vec<&Vec<String>> = reached.parts.iter().map(|(inline, _)| inline).collect();
        // A list of the test's file in one of its parts, and one that holds a part.
        let within = |inline: &[String]| parts.iter().any(|part| inline.starts_with(part));
        let around = |inline: &[String]| parts.iter().any(|part| part.starts_with(inline));
        let trees: Vec<&ModuleTree> = ats
            .iter()
            .filter_map(|At(at)| self.trees.get(*at))
            .collect();
        // The paths of the test's file as a module of each target (a file can be a
        // module of one target more than once).
        let file_modules: Vec<Vec<ModulePath>> = trees
            .iter()
            .map(|tree| {
                tree.modules
                    .iter()
                    .filter(|module| module.file == *file)
                    .filter(|module| module.list().is_some_and(|list| list.inline.is_empty()))
                    .map(|module| module.path.clone())
                    .collect()
            })
            .collect();
        // The paths of the test's module in its targets: its file's module, then the
        // inline modules on the way to it.
        let at_paths: Vec<ModulePath> = file_modules
            .iter()
            .flatten()
            .map(|module| module.joined(&reached.module))
            .collect();
        let mut holding = Holding {
            trees: trees.clone(),
            named: trees.iter().map(|_| Vec::new()).collect(),
            names: reached.own.names.clone(),
            members: reached.own.members.clone(),
            includes: reached
                .own
                .includes
                .iter()
                .map(|call| (file.clone(), call.clone()))
                .collect(),
            held: BTreeMap::new(),
            items: BTreeMap::new(),
        };
        for (index, modules) in file_modules.iter().enumerate() {
            for module in modules {
                holding.resolve_in(index, &module.joined(&reached.module), &reached.own);
                for (inline, refs) in &reached.parts {
                    holding.resolve_in(index, &module.joined(inline), refs);
                }
            }
        }
        for (_, refs) in &reached.parts {
            holding.names.extend(refs.names.iter().cloned());
            holding.members.extend(refs.members.iter().cloned());
            holding.includes.extend(
                refs.includes
                    .iter()
                    .map(|call| (file.clone(), call.clone())),
            );
        }
        let mut by_name: Vec<Wait<'_>> = Vec::new();
        // The modules of its file a test may hold, besides those it is in or that are
        // in its parts: their items are names of it only once they are held.
        let others: Vec<&[String]> = trees
            .iter()
            .flat_map(|tree| &tree.modules)
            .filter(|module| module.file == *file)
            .filter_map(|module| module.list())
            .filter(|list| list.unit.is_some())
            .map(|list| list.inline.as_slice())
            .filter(|inline| !within(inline) && !around(inline))
            .collect();
        for (index, tree) in trees.iter().enumerate() {
            for module in &tree.modules {
                let Some(list) = module.list() else {
                    continue;
                };
                let own = module.file == *file;
                // The items of the modules compiled only under test of its file, outside
                // its parts and outside the modules of it a test may hold (a residue
                // item is a name of it once it is held, as any other); each path in
                // them is read from the module of its list.
                if own
                    && list.compiled == Compiled::OnlyUnderTest
                    && !within(&list.inline)
                    && !others.iter().any(|other| list.inline.starts_with(other))
                {
                    holding.names.extend(list.test_refs.names.iter().cloned());
                    holding
                        .members
                        .extend(list.test_refs.members.iter().cloned());
                    holding.resolve_in(index, &module.path, &list.test_refs);
                }
                if list.compiled == Compiled::Always && !(own && within(&list.inline)) {
                    for (position, item) in list.residue.iter().enumerate() {
                        let in_subtree = || {
                            at_paths
                                .iter()
                                .any(|path| path.0.starts_with(&module.path.0))
                        };
                        match item.kind {
                            ResidueKind::Everywhere => {
                                holding.hold_item(index, module, position, item)
                            }
                            ResidueKind::Use if in_subtree() => {
                                holding.hold_item(index, module, position, item)
                            }
                            ResidueKind::Use | ResidueKind::ByName => by_name.push(Wait::Item {
                                tree: index,
                                module,
                                position,
                                item,
                            }),
                        }
                    }
                }
                if let Some(unit) = &list.unit
                    && let Some(unit_text) = &unit.text
                    && !(own && (within(&list.inline) || around(&list.inline)))
                {
                    match tree.reach(module) {
                        Reach::Everywhere(_) => holding.hold(index, module, unit_text, &unit.refs),
                        Reach::ByName(name) => by_name.push(Wait::Module {
                            tree: index,
                            name,
                            module,
                            unit,
                            text: unit_text,
                        }),
                    }
                }
            }
        }
        // Closed over what each module and residue item held names: each once.
        loop {
            let before = by_name.len();
            let mut at = 0;
            while at < by_name.len() {
                if by_name[at].reached(&holding) {
                    match by_name.swap_remove(at) {
                        Wait::Module {
                            tree,
                            module,
                            unit,
                            text,
                            ..
                        } => holding.hold(tree, module, text, &unit.refs),
                        Wait::Item {
                            tree,
                            module,
                            position,
                            item,
                        } => holding.hold_item(tree, module, position, item),
                    }
                } else {
                    at += 1;
                }
            }
            if by_name.len() == before {
                break;
            }
        }
        // A module that cannot be read, reached by a path of the text or standing under
        // `#[macro_use]`.
        for (index, tree) in trees.iter().enumerate() {
            for missing in &tree.unresolved {
                if missing.under == MacroUse::Under
                    || holding.named[index]
                        .iter()
                        .any(|named| named.reaches(&missing.path, missing.module()))
                {
                    return Err(Untellable::Helper {
                        at: missing.at.clone(),
                        why: format!(
                            "the module `{}` declared there cannot be read: {}",
                            missing.module(),
                            missing.why
                        ),
                        told: holding.told(file),
                    });
                }
            }
        }
        let mut includes: BTreeMap<RelPath, [u8; 32]> = BTreeMap::new();
        for (from, call) in &holding.includes {
            let cannot = |why: String| Untellable::Helper {
                at: from.clone(),
                why,
                told: holding.told(file),
            };
            let path = match call {
                IncludeCall::Unread(why) => return Err(cannot(why.clone())),
                IncludeCall::Literal(text) => {
                    let mut dir: Vec<String> =
                        from.as_str().split('/').map(str::to_owned).collect();
                    dir.pop();
                    match joined(&dir, text) {
                        Ok(path) => path,
                        Err(why) => {
                            return Err(cannot(format!("the path `{text}` of an include {why}")));
                        }
                    }
                }
            };
            if includes.contains_key(&path) {
                continue;
            }
            let digest = self.digest(space, side, &path).await.map_err(&cannot)?;
            includes.insert(path, digest);
        }
        let mut parts = holding.held;
        for (key, items) in holding.items {
            let lines: Vec<&str> = items.values().map(|text| &**text).collect();
            parts
                .entry(key)
                .or_insert_with(|| Arc::from(lines.join("\n")));
        }
        Ok(Held {
            modules: parts
                .into_iter()
                .map(|((file, module), text)| HeldModule { module, file, text })
                .collect(),
            includes: includes
                .into_iter()
                .map(|(path, sha256)| Include { path, sha256 })
                .collect(),
        })
    }

    /// The SHA-256 of the file at `path` an include reads, or why it cannot be told:
    /// read and hashed once for every test that holds it.
    async fn digest(
        &self,
        space: &RustTestSpace,
        side: Side,
        path: &RelPath,
    ) -> Result<[u8; 32], String> {
        if let Some(done) = self.digests_of().get(path) {
            return done.clone();
        }
        let digest = match space.source_at(side, path, space.lister.max_file()).await {
            Source::Bytes(bytes) => {
                super::super::on_blocking(bytes.len(), move || {
                    <[u8; 32]>::from(Sha256::digest(&bytes))
                })
                .await
            }
            Source::NoFile => Err(format!(
                "{path}, which an include there reads, is not there"
            )),
            Source::Untold(why) => Err(why.to_string()),
        };
        self.digests_of().insert(path.clone(), digest.clone());
        digest
    }

    /// The digests made so far; one poisoned by a panic is taken as it is.
    fn digests_of(&self) -> std::sync::MutexGuard<'_, HashMap<RelPath, Result<[u8; 32], String>>> {
        self.digests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A module or an item of a residue a test may still hold, of the tree at `tree` among
/// those of the test's targets.
enum Wait<'a> {
    Module {
        tree: usize,
        name: String,
        module: &'a Module,
        unit: &'a Unit,
        text: &'a Arc<str>,
    },
    Item {
        tree: usize,
        module: &'a Module,
        position: usize,
        item: &'a ResidueItem,
    },
}

impl Wait<'_> {
    /// Whether a path held names the module (`Named::reaches`, in its tree), or an
    /// identifier of one of the names of the item is held (R2b keeps its rule by
    /// identifier).
    fn reached(&self, holding: &Holding<'_>) -> bool {
        match self {
            Self::Module {
                tree, name, module, ..
            } => holding.named[*tree]
                .iter()
                .any(|named| named.reaches(&module.path, name)),
            Self::Item { item, .. } => item.via.reached(&holding.names, &holding.members),
        }
    }
}

/// What one test holds, as it is gathered: the identifiers its text and what it holds
/// so far stand in (for the residue items, R2b), what each path of them names in each
/// tree of the test's targets (for the modules and for R5), the includes of them with
/// the file of each, the modules held by `(file, module)`, and the residue items held,
/// by the same key and their place in the file.
struct Holding<'a> {
    trees: Vec<&'a ModuleTree>,
    named: Vec<Vec<Named<'a>>>,
    names: BTreeSet<String>,
    /// The member names of the same texts (R2b's `impl` and `trait` items).
    members: BTreeSet<String>,
    includes: BTreeSet<(RelPath, IncludeCall)>,
    held: BTreeMap<(RelPath, ModulePath), Arc<str>>,
    items: BTreeMap<(RelPath, ModulePath), BTreeMap<usize, Arc<str>>>,
}

impl<'a> Holding<'a> {
    /// What each path of `refs`, written in the module at `base`, names in the tree at
    /// `tree`.
    fn resolve_in(&mut self, tree: usize, base: &ModulePath, refs: &Refs) {
        let of: &'a ModuleTree = self.trees[tree];
        self.named[tree].extend(refs.paths.iter().map(|path| resolve(of, base, path)));
    }

    /// Holds the residue item `item`, the `position`th of `module` of the tree at
    /// `tree`, once, and what it names.
    fn hold_item(&mut self, tree: usize, module: &Module, position: usize, item: &ResidueItem) {
        let held = self
            .items
            .entry((module.file.clone(), module.path.clone()))
            .or_default();
        if held.contains_key(&position) {
            return;
        }
        held.insert(position, item.text.clone());
        self.take_in(tree, module, &item.refs);
    }

    /// Takes in what `refs`, held from `module` of the tree at `tree`, name: the
    /// identifiers, the member names, what each path names there, and the includes.
    fn take_in(&mut self, tree: usize, module: &Module, refs: &Refs) {
        self.names.extend(refs.names.iter().cloned());
        self.members.extend(refs.members.iter().cloned());
        self.resolve_in(tree, &module.path, refs);
        self.includes.extend(
            refs.includes
                .iter()
                .map(|call| (module.file.clone(), call.clone())),
        );
    }

    /// Holds the part `text` of `module` of the tree at `tree`, once, and what `refs` of
    /// it name.
    fn hold(&mut self, tree: usize, module: &Module, text: &Arc<str>, refs: &Refs) {
        let key = (module.file.clone(), module.path.clone());
        if self.held.contains_key(&key) {
            return;
        }
        self.held.insert(key, text.clone());
        self.take_in(tree, module, refs);
    }

    /// The files told: the test's own, `file`, and those of the parts held so far.
    fn told(&self, file: &RelPath) -> Vec<RelPath> {
        std::iter::once(file.clone())
            .chain(self.held.keys().map(|(file, _)| file.clone()))
            .chain(self.items.keys().map(|(file, _)| file.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use whyshelf_core::app::shelf::NamePlaces;
    use whyshelf_core::domain::nonempty::NonEmptySet;
    use whyshelf_core::domain::shelf::artifact::{
        ArtifactName, ArtifactState, Comparison, SpaceAsks, Untold,
    };
    use whyshelf_core::domain::shelf::placement::Placement;

    use super::super::RustTestSpace;
    use super::*;

    /// The manifest of a package `demo` with a library at `src/lib.rs`.
    const PACKAGE: &str = "[package]\nname = \"demo\"\nedition = \"2021\"\n";

    fn n(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    async fn write(dir: &Path, at: &str, text: &str) {
        let full = dir.join(at);
        if let Some(parent) = full.parent() {
            // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
        tokio::fs::write(full, text).await.unwrap();
    }

    /// A project of `files` under a new root.
    async fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (at, text) in files {
            write(dir.path(), at, text).await;
        }
        dir
    }

    /// The text the test `name` reads as under `root`; it is there.
    async fn text(root: &Path, name: &str) -> String {
        let bytes = RustTestSpace::new(root.to_path_buf())
            .current(&n(name))
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{name} has a text"));
        String::from_utf8(bytes).unwrap()
    }

    /// Why the test `name` cannot be told under `space`.
    async fn why_in(space: &RustTestSpace, name: &str) -> String {
        let asks: SpaceAsks = [(n(name), BTreeSet::from([Comparison::Bytes]))]
            .into_iter()
            .collect();
        let found = space.resolve(&asks).await.unwrap();
        match found[&n(name)].state(&Comparison::Bytes) {
            ArtifactState::Unknown(Untold::Said(why)) => why.as_str().to_owned(),
            other => panic!("{name}: {other:?}"),
        }
    }

    async fn why(root: &Path, name: &str) -> String {
        why_in(&RustTestSpace::new(root.to_path_buf()), name).await
    }

    /// Whether `text` holds the module `module` of the file `file`.
    fn holds(text: &str, file: &str, module: &str) -> bool {
        text.contains(&format!("\n@ {file} {module}\n"))
    }

    /// The modules `text` holds, as `<file> <module>`, in the order of the text.
    fn held(text: &str) -> Vec<String> {
        text.lines()
            .filter_map(|line| line.strip_prefix("@ "))
            .map(str::to_owned)
            .collect()
    }

    /// A library whose tests name a helper module compiled only under test, or not.
    const LIB: &str = "pub fn code() -> u8 { 1 }\n\n#[cfg(test)]\nmod fixtures_tests;\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn names_the_helper() { assert_eq!(crate::fixtures_tests::seed(), code()); }\n\n    #[test]\n    fn names_nothing() { assert_eq!(code(), 1); }\n}\n";

    const FIXTURES: &str = "pub fn seed() -> u8 { 1 }\n";

    #[tokio::test]
    async fn a_test_naming_a_helper_holds_it_and_one_that_does_not_holds_nothing() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            ("src/lib.rs", LIB),
            ("src/fixtures_tests.rs", FIXTURES),
        ])
        .await;
        let r = dir.path();
        let named = text(r, "src/lib.rs::tests::names_the_helper").await;
        assert!(
            named.ends_with(
                "\n\n@ src/fixtures_tests.rs crate::fixtures_tests\npub fn seed () -> u8 { 1 }"
            ),
            "{named}"
        );
        let nothing = text(r, "src/lib.rs::tests::names_nothing").await;
        assert!(!nothing.contains("\n@"), "{nothing}");
        // An edit of the helper changes the text of the test that names it only.
        write(r, "src/fixtures_tests.rs", "pub fn seed() -> u8 { 2 }\n").await;
        assert_ne!(text(r, "src/lib.rs::tests::names_the_helper").await, named);
        assert_eq!(text(r, "src/lib.rs::tests::names_nothing").await, nothing);
        // An edit of its docs or spacing changes nothing.
        write(
            r,
            "src/fixtures_tests.rs",
            "/// Two.\npub fn seed()->u8{2}\n",
        )
        .await;
        let respaced = text(r, "src/lib.rs::tests::names_the_helper").await;
        write(r, "src/fixtures_tests.rs", "pub fn seed() -> u8 { 2 }\n").await;
        assert_eq!(
            text(r, "src/lib.rs::tests::names_the_helper").await,
            respaced
        );
    }

    #[tokio::test]
    async fn a_test_that_holds_nothing_keeps_the_text_it_had_with_no_crate_read() {
        let dir = project(&[("src/lib.rs", LIB), ("src/fixtures_tests.rs", FIXTURES)]).await;
        let r = dir.path();
        // No manifest: no crate, nothing held, for either test.
        let bare = text(r, "src/lib.rs::tests::names_nothing").await;
        let bare_named = text(r, "src/lib.rs::tests::names_the_helper").await;
        assert!(!bare_named.contains("\n@"), "{bare_named}");
        write(r, "Cargo.toml", PACKAGE).await;
        assert_eq!(text(r, "src/lib.rs::tests::names_nothing").await, bare);
        let space = RustTestSpace::new(r.to_path_buf());
        let asks: SpaceAsks = [(
            n("src/lib.rs::tests::names_nothing"),
            BTreeSet::from([Comparison::Bytes]),
        )]
        .into_iter()
        .collect();
        let found = space.resolve(&asks).await.unwrap();
        assert_eq!(
            found[&n("src/lib.rs::tests::names_nothing")],
            whyshelf_core::domain::shelf::artifact::ArtifactReading::of_bytes(
                bare.as_bytes(),
                &BTreeSet::from([Comparison::Bytes]),
                None
            )
        );
        // A path holds no line break, so a line of what is held is one path.
        assert!("a\nb.rs".parse::<RelPath>().is_err());
        assert!("a\rb.rs".parse::<RelPath>().is_err());
    }

    #[tokio::test]
    async fn what_a_held_module_names_is_held_once_and_a_test_holds_no_module_it_is_in() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            (
                "src/lib.rs",
                "pub fn code() -> u8 { 1 }\n#[cfg(test)]\nmod fixtures_tests;\n#[cfg(test)]\nmod more_tests;\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { crate::fixtures_tests::seed(); }\n}\n",
            ),
            (
                "src/fixtures_tests.rs",
                "pub fn seed() -> u8 { crate::more_tests::one() }\n#[test]\nfn inside() { seed(); }\n",
            ),
            (
                "src/more_tests.rs",
                "pub fn one() -> u8 { 1 }\npub fn back() -> u8 { crate::fixtures_tests::seed() }\n",
            ),
        ])
        .await;
        let r = dir.path();
        // A → B, B → A: each once, in the order of the files.
        assert_eq!(
            held(&text(r, "src/lib.rs::tests::t").await),
            [
                "src/fixtures_tests.rs crate::fixtures_tests",
                "src/more_tests.rs crate::more_tests"
            ]
        );
        // A test in a helper holds what it names, and not the module it is in.
        assert_eq!(
            held(&text(r, "src/fixtures_tests.rs::inside").await),
            ["src/more_tests.rs crate::more_tests"]
        );
    }

    #[tokio::test]
    async fn the_file_of_a_module_is_found_by_the_rules_of_the_reference() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            (
                "src/lib.rs",
                "#[cfg(test)]\nmod a_tests;\n#[cfg(test)]\n#[path = \"elsewhere/b.rs\"]\nmod b_tests;\n#[cfg(test)]\nmod c_tests;\nmod code;\nmod f_tests;\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        a_tests::a();\n        b_tests::b();\n        c_tests::c();\n        c_tests::deep_helpers::x();\n        code::inline::d_tests::d();\n        code::e_tests::e();\n        f_tests::f();\n    }\n}\n",
            ),
            ("src/a_tests.rs", "pub fn a() {}\n"),
            ("src/elsewhere/b.rs", "pub fn b() {}\n"),
            ("src/c_tests/mod.rs", "pub fn c() {}\npub mod deep_helpers;\n"),
            ("src/c_tests/deep_helpers.rs", "pub fn x() {}\n"),
            (
                "src/code.rs",
                "pub fn f() {}\n#[cfg(test)]\npub mod inline {\n    #[path = \"d.rs\"]\n    pub mod d_tests;\n}\n#[cfg(test)]\npub mod e_tests;\n",
            ),
            ("src/code/inline/d.rs", "pub fn d() {}\n"),
            ("src/code/e_tests.rs", "pub fn e() {}\n"),
            ("src/f_tests.rs", "#![cfg(test)]\npub fn f() {}\n"),
        ])
        .await;
        let held = held(&text(dir.path(), "src/lib.rs::tests::t").await);
        assert_eq!(
            held,
            [
                "src/a_tests.rs crate::a_tests",
                "src/c_tests/deep_helpers.rs crate::c_tests::deep_helpers",
                "src/c_tests/mod.rs crate::c_tests",
                "src/code.rs crate::code::inline",
                "src/code/e_tests.rs crate::code::e_tests",
                "src/code/inline/d.rs crate::code::inline::d_tests",
                "src/elsewhere/b.rs crate::b_tests",
                "src/f_tests.rs crate::f_tests",
            ]
        );
    }

    #[tokio::test]
    async fn a_module_is_compiled_only_under_test_by_a_cfg_that_holds_only_under_test() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            (
                "src/lib.rs",
                "#[cfg(test)]\nmod t1;\n#[cfg(all(test, unix))]\nmod t2;\n#[cfg(any(test, feature = \"x\"))]\nmod t3;\n#[cfg(not(test))]\nmod t4;\n#[cfg(feature = \"test\")]\nmod t5;\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { crate::t1::f(); crate::t2::f(); crate::t3::f(); crate::t4::f(); crate::t5::f(); }\n}\n",
            ),
            ("src/t1.rs", "pub fn f() {}\n"),
            ("src/t2.rs", "pub fn f() {}\n"),
            ("src/t3.rs", "pub fn f() {}\n"),
            ("src/t4.rs", "pub fn f() {}\n"),
            ("src/t5.rs", "pub fn f() {}\n"),
        ])
        .await;
        assert_eq!(
            held(&text(dir.path(), "src/lib.rs::tests::t").await),
            ["src/t1.rs crate::t1", "src/t2.rs crate::t2"]
        );
    }

    #[tokio::test]
    async fn an_alias_a_reexport_and_a_glob_name_a_helper_and_the_code_under_test_is_never_held() {
        let lib = |part: &str| {
            format!(
                "pub fn code() -> u8 {{ 1 }}\n#[cfg(test)]\nmod fixtures_tests;\n#[cfg(test)]\npub mod support {{\n    pub use crate::fixtures_tests::seed;\n}}\n#[cfg(test)]\nmod tests {{\n    use super::*;\n{part}\n    #[test]\n    fn t() {{ code(); }}\n}}\n"
            )
        };
        for (part, test, held_now) in [
            (
                "    use crate::fixtures_tests as fx;\n    #[test]\n    fn by_alias() { fx::seed(); }\n",
                "by_alias",
                vec!["src/fixtures_tests.rs crate::fixtures_tests"],
            ),
            (
                "    use crate::fixtures_tests::*;\n    #[test]\n    fn by_glob() { seed(); }\n",
                "by_glob",
                vec!["src/fixtures_tests.rs crate::fixtures_tests"],
            ),
            // Through a module that re-exports it, closed over what that one names.
            (
                "    #[test]\n    fn by_reexport() { crate::support::seed(); }\n",
                "by_reexport",
                vec![
                    "src/fixtures_tests.rs crate::fixtures_tests",
                    "src/lib.rs crate::support",
                ],
            ),
        ] {
            let dir = project(&[
                ("Cargo.toml", PACKAGE),
                ("src/lib.rs", &lib(part)),
                ("src/fixtures_tests.rs", FIXTURES),
            ])
            .await;
            assert_eq!(
                held(&text(dir.path(), &format!("src/lib.rs::tests::{test}")).await),
                held_now,
                "{part}"
            );
        }
        // `use super::*` reaching the code under test holds nothing: an edit of the
        // code under test keeps the text.
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            ("src/lib.rs", &lib("")),
            ("src/fixtures_tests.rs", FIXTURES),
        ])
        .await;
        let before = text(dir.path(), "src/lib.rs::tests::t").await;
        assert!(!before.contains("\n@"), "{before}");
        write(dir.path(), "src/lib.rs", &lib("").replace("{ 1 }", "{ 2 }")).await;
        assert_eq!(text(dir.path(), "src/lib.rs::tests::t").await, before);
    }

    #[tokio::test]
    async fn a_macro_exported_a_macro_use_and_an_impl_of_a_type_elsewhere_are_held_by_every_test() {
        let lib = |decl: &str| {
            format!(
                "pub struct Thing;\n{decl}\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn t() {{}}\n}}\n"
            )
        };
        for (decl, file, body) in [
            (
                "#[cfg(test)]\nmod macros_tests;",
                "src/macros_tests.rs",
                "#[macro_export]\nmacro_rules! make { () => { 1 } }\n",
            ),
            (
                "#[cfg(test)]\n#[macro_use]\nmod used_tests;",
                "src/used_tests.rs",
                "macro_rules! m { () => {} }\npub fn x() {}\n",
            ),
            (
                "#[cfg(test)]\nmod impls_tests;",
                "src/impls_tests.rs",
                "impl crate::Thing {\n    pub fn seeded() -> Self { crate::Thing }\n}\n",
            ),
        ] {
            let dir = project(&[
                ("Cargo.toml", PACKAGE),
                ("src/lib.rs", &lib(decl)),
                (file, body),
            ])
            .await;
            let text = text(dir.path(), "src/lib.rs::tests::t").await;
            assert_eq!(held(&text).len(), 1, "{decl}: {text}");
            assert!(held(&text)[0].starts_with(file), "{decl}: {text}");
        }
        // An impl of a type the module declares is reached by its name only.
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            ("src/lib.rs", &lib("#[cfg(test)]\nmod own_tests;")),
            (
                "src/own_tests.rs",
                "pub struct Fixture;\nimpl Fixture { pub fn new() -> Self { Fixture } }\n",
            ),
        ])
        .await;
        assert!(held(&text(dir.path(), "src/lib.rs::tests::t").await).is_empty());
    }

    /// Whether the test `t` of the module `tests` of `src/other.rs`, with `part` beside
    /// it in that module and `body` as its body, holds the residue of `src/lib.rs`.
    async fn holds_residue(lib: &str, part: &str, body: &str) -> bool {
        let other = format!(
            "#[cfg(test)]\nmod tests {{\n{part}\n    #[test]\n    fn t() {{ {body} }}\n}}\n"
        );
        let files = [("src/lib.rs", lib), ("src/other.rs", other.as_str())];
        held_in(&files, "src/other.rs::tests::t")
            .await
            .contains(&"src/lib.rs crate".to_owned())
    }

    const MEMBERS_LIB: &str = "pub struct X;\npub struct Z;\n#[cfg(test)]\nimpl X {\n    pub const EMPTY: u8 = 0;\n    pub fn approval(&self) -> u8 { 1 }\n    pub fn m(&self) -> u8 { 2 }\n}\nmod other;\n";

    #[tokio::test]
    async fn an_impl_of_the_residue_is_held_by_a_name_in_member_position_not_by_a_local() {
        // A local or a parameter of the name is no member name.
        for (part, body) in [
            ("", "let approval = 1u8; let _ = approval.pow(2);"),
            ("fn f(approval: u8) -> u8 { approval }", "f(1);"),
            (
                "",
                "let m = 1; assert_eq!(m, 1); assert_eq!(approval(), 1);",
            ),
        ] {
            assert!(
                !holds_residue(MEMBERS_LIB, part, body).await,
                "{part} {body}"
            );
        }
        // A method call, a path to the item, a turbofish, a qualified path, a name
        // passed as a function, a constant in a pattern, a macro's body.
        for (part, body) in [
            ("", "let x = crate::X; x.approval();"),
            ("", "let x = crate::X; x.approval::<u8>();"),
            ("", "let x = crate::X; let _ = crate::X::approval(&x);"),
            ("", "let x = crate::X; let _ = <crate::X>::approval(&x);"),
            ("", "let x = crate::X; let _ = <crate::X as Tr>::m(&x);"),
            ("", "let _ = [crate::X].iter().map(crate::X::m);"),
            ("", "match 0u8 { crate::X::EMPTY => {} _ => {} }"),
            (
                "macro_rules! call { ($x:expr) => { $x.m() } }",
                "call!(crate::X);",
            ),
            // A name written as an argument of a macro that may build a call from it.
            ("", "check!(x, m);"),
        ] {
            assert!(
                holds_residue(MEMBERS_LIB, part, body).await,
                "{part} {body}"
            );
        }
    }

    #[tokio::test]
    async fn a_member_name_in_what_is_held_reaches_an_impl_and_a_trait_by_its_items() {
        // `Self::m()` in a held item is a member name: the closure reaches the impl of m.
        let lib = "pub struct X;\npub struct Z;\n#[cfg(test)]\nimpl X {\n    pub fn m() {}\n}\n#[cfg(test)]\nimpl Z {\n    pub fn z() { Self::m() }\n}\nmod other;\n";
        let other = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { crate::Z::z(); }\n}\n";
        let files = [("src/lib.rs", lib), ("src/other.rs", other)];
        let said = text_of(&files, "src/other.rs::tests::t").await;
        assert!(said.contains("impl Z") && said.contains("impl X"), "{said}");
        // A trait by its default method as a member, or by its own name; a local of the
        // method's name holds nothing.
        let lib =
            "#[cfg(test)]\npub trait Probe {\n    fn probe(&self) -> u8 { 1 }\n}\nmod other;\n";
        for (body, held_now) in [
            ("check!(x, probe);", true),
            ("let x = 1u8; let _ = x.probe();", true),
            ("let probe = 1; assert_eq!(probe, 1);", false),
            ("fn_with(Probe);", true),
        ] {
            assert_eq!(holds_residue(lib, "", body).await, held_now, "{body}");
        }
    }

    /// The text of the test `name` of the package `demo` made of `files`.
    async fn text_of(files: &[(&str, &str)], name: &str) -> String {
        let mut all = vec![("Cargo.toml", PACKAGE)];
        all.extend_from_slice(files);
        let dir = project(&all).await;
        text(dir.path(), name).await
    }

    #[tokio::test]
    async fn an_item_of_the_test_residue_is_held_by_the_tests_that_name_it() {
        let lib = "pub struct Shelf;\n#[cfg(test)]\nimpl Shelf {\n    pub fn seeded() -> Self { Shelf }\n}\n#[cfg(test)]\nfn helper() -> u8 { crate::fixtures_tests::seed() }\n#[cfg(test)]\npub trait Probe {\n    fn probe(&self) -> u8 { 1 }\n}\n#[cfg(test)]\nmod fixtures_tests;\n#[cfg(test)]\n#[path = \"fx.rs\"]\nmod other_tests;\nmod other;\n";
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            ("src/lib.rs", lib),
            ("src/fixtures_tests.rs", FIXTURES),
            ("src/fx.rs", "pub fn other() {}\n"),
            (
                "src/other.rs",
                "#[cfg(test)]\nmod tests {\n    #[test]\n    fn calls() { crate::helper(); }\n    #[test]\n    fn seeds() { let _ = crate::Shelf::seeded(); }\n    #[test]\n    fn probes() { let x = (); let _ = x.probe(); }\n    #[test]\n    fn both() { crate::helper(); crate::Shelf::seeded(); }\n    #[test]\n    fn none() {}\n}\n",
            ),
        ])
        .await;
        let r = dir.path();
        let of = |name: &str| format!("src/other.rs::tests::{name}");
        // A `fn` by a test of another file that calls it, closed over what it names.
        let calls = text(r, &of("calls")).await;
        assert_eq!(
            held(&calls),
            [
                "src/fixtures_tests.rs crate::fixtures_tests",
                "src/lib.rs crate"
            ]
        );
        assert!(
            calls.ends_with(
                "\n@ src/lib.rs crate\n# [cfg (test)] fn helper () -> u8 { crate :: fixtures_tests :: seed () }"
            ),
            "{calls}"
        );
        // An inherent `impl` by the name of a method; a `trait` by its default method.
        let seeds = text(r, &of("seeds")).await;
        assert_eq!(held(&seeds), ["src/lib.rs crate"]);
        assert!(
            seeds.ends_with(
                "\n\n@ src/lib.rs crate\n# [cfg (test)] impl Shelf { pub fn seeded () -> Self { Shelf } }"
            ),
            "{seeds}"
        );
        let probes = text(r, &of("probes")).await;
        assert_eq!(held(&probes), ["src/lib.rs crate"]);
        assert!(probes.contains("trait Probe"), "{probes}");
        assert!(!probes.contains("impl Shelf"), "{probes}");
        // The items held of one module are one part, in the order of the file.
        let both = text(r, &of("both")).await;
        let (shelf, helper) = (
            both.find("impl Shelf").unwrap(),
            both.find("fn helper").unwrap(),
        );
        assert!(shelf < helper, "{both}");
        assert_eq!(both.matches("\n@ src/lib.rs crate\n").count(), 1, "{both}");
        // A test that names none holds none; modules declared under `#[cfg(test)]`
        // are no residue.
        let none = text(r, &of("none")).await;
        assert!(held(&none).is_empty(), "{none}");
        // An edit of a held item makes stale exactly the tests that hold it.
        let before: Vec<String> = {
            let mut texts = Vec::new();
            for name in ["calls", "seeds", "probes", "none"] {
                texts.push(text(r, &of(name)).await);
            }
            texts
        };
        write(r, "src/lib.rs", &lib.replace("{ Shelf }", "{ Shelf {} }")).await;
        for (name, before, changed) in [
            ("calls", &before[0], false),
            ("seeds", &before[1], true),
            ("probes", &before[2], false),
            ("none", &before[3], false),
        ] {
            assert_eq!(&text(r, &of(name)).await != before, changed, "{name}");
        }
    }

    #[tokio::test]
    async fn an_item_of_the_test_residue_no_name_reaches_is_held_by_every_test() {
        for residue in [
            // A trait `impl`.
            "#[cfg(test)]\nimpl Clone for Thing {\n    fn clone(&self) -> Self { Thing }\n}",
            // A derive that holds only under test, on an item of code.
            "#[cfg_attr(test, derive(Debug))]\npub struct Other;",
            // An `impl` with no associated item.
            "#[cfg(test)]\nimpl Thing {}",
            // An item macro call.
            "#[cfg(test)]\nthread_local! { static X: u8 = 0; }",
        ] {
            let dir = project(&[
                ("Cargo.toml", PACKAGE),
                (
                    "src/lib.rs",
                    &format!("pub struct Thing;\n{residue}\nmod other;\n"),
                ),
                (
                    "src/other.rs",
                    "#[cfg(test)]\nmod tests {\n    #[test]\n    fn none() {}\n}\n",
                ),
            ])
            .await;
            assert_eq!(
                held(&text(dir.path(), "src/other.rs::tests::none").await),
                ["src/lib.rs crate"],
                "{residue}"
            );
        }
    }

    #[tokio::test]
    async fn a_use_of_the_test_residue_reaches_its_subtree_and_a_public_one_its_names() {
        let a_tests = "use super::*;\n#[test]\nfn t() {}\n";
        let b = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn u() {}\n    #[test]\n    fn names() { crate::a::seed(); }\n}\n";
        let lib = "#[cfg(test)]\nmod fixtures_tests;\nmod a;\nmod b;\n";
        let at = |a: &str| {
            [
                ("Cargo.toml", PACKAGE),
                ("src/lib.rs", lib),
                ("src/fixtures_tests.rs", FIXTURES),
                ("src/a_tests.rs", a_tests),
                ("src/b.rs", b),
            ]
            .into_iter()
            .map(|(p, t)| (p.to_owned(), t.to_owned()))
            .chain(std::iter::once(("src/a.rs".to_owned(), a.to_owned())))
            .collect::<Vec<_>>()
        };
        let both = [
            "src/a.rs crate::a",
            "src/fixtures_tests.rs crate::fixtures_tests",
        ];
        for (a, t, u, names) in [
            // A private `use` in `a`: the tests of its subtree, in another file by
            // `#[path]`, and not those of a sibling.
            (
                "pub fn code() {}\n#[cfg(test)]\nuse crate::fixtures_tests::*;\n#[cfg(test)]\n#[path = \"a_tests.rs\"]\nmod tests;\n",
                &both[..],
                &[][..],
                &[][..],
            ),
            // A `pub(crate)` one: also a test that names what it brings in.
            (
                "#[cfg(test)]\npub(crate) use crate::fixtures_tests::seed;\n#[cfg(test)]\n#[path = \"a_tests.rs\"]\nmod tests;\n",
                &both[..],
                &[][..],
                &both[..],
            ),
            // A public glob: the module it brings in is held by every test.
            (
                "#[cfg(test)]\npub use crate::fixtures_tests::*;\n#[cfg(test)]\n#[path = \"a_tests.rs\"]\nmod tests;\n",
                &both[..],
                &["src/fixtures_tests.rs crate::fixtures_tests"][..],
                &["src/fixtures_tests.rs crate::fixtures_tests"][..],
            ),
        ] {
            let files = at(a);
            let files: Vec<(&str, &str)> = files
                .iter()
                .map(|(p, t)| (p.as_str(), t.as_str()))
                .collect();
            let dir = project(&files).await;
            let r = dir.path();
            assert_eq!(held(&text(r, "src/a_tests.rs::t").await), t, "{a}");
            assert_eq!(held(&text(r, "src/b.rs::tests::u").await), u, "{a}");
            assert_eq!(held(&text(r, "src/b.rs::tests::names").await), names, "{a}");
        }
    }

    #[tokio::test]
    async fn two_modules_of_one_file_are_two_parts_and_a_module_of_tests_only_is_none() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            (
                "src/lib.rs",
                "mod helpers;\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { crate::helpers::beta::b(); crate::helpers::alpha::a(); crate::helpers::only_tests::x; }\n}\n",
            ),
            (
                "src/helpers.rs",
                "pub fn code() {}\n#[cfg(test)]\npub mod alpha { pub fn a() {} }\n#[cfg(test)]\npub mod beta { pub fn b() {} }\n#[cfg(test)]\npub mod only_tests { #[test] fn x() {} }\n",
            ),
        ])
        .await;
        let text = text(dir.path(), "src/lib.rs::tests::t").await;
        assert_eq!(
            held(&text),
            [
                "src/helpers.rs crate::helpers::alpha",
                "src/helpers.rs crate::helpers::beta"
            ]
        );
        assert!(holds(&text, "src/helpers.rs", "crate::helpers::alpha"));
    }

    #[tokio::test]
    async fn a_file_in_two_targets_holds_what_it_names_of_each() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            ("src/lib.rs", "mod shared;\n#[cfg(test)]\nmod lib_tests;\n"),
            (
                "src/main.rs",
                "mod shared;\n#[cfg(test)]\nmod bin_tests;\nfn main() {}\n",
            ),
            ("src/lib_tests.rs", "pub fn x() {}\n"),
            ("src/bin_tests.rs", "pub fn y() {}\n"),
            (
                "src/shared.rs",
                "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { crate::lib_tests::x(); crate::bin_tests::y(); }\n}\n",
            ),
            (
                "tests/a.rs",
                "mod common;\n#[test]\nfn a() { common::help(); }\n",
            ),
            ("tests/b.rs", "mod common;\n#[test]\nfn b() {}\n"),
            (
                "tests/c.rs",
                "mod common;\nfn helper() { common::help(); }\n#[cfg(test)]\nmod inner {\n    #[test]\n    fn c() { super::helper(); }\n}\n",
            ),
            (
                "tests/common/mod.rs",
                "pub fn help() {}\n#[test]\nfn in_common() {}\n",
            ),
        ])
        .await;
        let r = dir.path();
        assert_eq!(
            held(&text(r, "src/shared.rs::tests::t").await),
            [
                "src/bin_tests.rs crate::bin_tests",
                "src/lib_tests.rs crate::lib_tests"
            ]
        );
        // `tests/common/mod.rs` of two integration tests: a test naming it holds it,
        // one that does not, does not.
        assert_eq!(
            held(&text(r, "tests/a.rs::a").await),
            ["tests/common/mod.rs crate::common"]
        );
        assert!(held(&text(r, "tests/b.rs::b").await).is_empty());
        // An item of its file outside its part, compiled only under test, names it
        // too.
        assert_eq!(
            held(&text(r, "tests/c.rs::inner::c").await),
            ["tests/common/mod.rs crate::common"]
        );
        // A test in it is of all three, and holds the root of each, which it reaches
        // as `crate` and `super`.
        assert_eq!(
            held(&text(r, "tests/common/mod.rs::in_common").await),
            ["tests/a.rs crate", "tests/b.rs crate", "tests/c.rs crate"]
        );
    }

    #[tokio::test]
    async fn an_include_with_a_literal_path_is_held_by_its_hash_and_another_cannot_be_told() {
        let lib = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn reads() { let _ = include_str!(\"data/x.txt\"); }\n    #[test]\n    fn concat() { let _ = include_str!(concat!(\"data/\", \"x.txt\")); }\n}\n";
        // The tests of a part share its items, so each is put in a part of its own.
        let split = lib.replace(
            "    #[test]\n    fn concat()",
            "}\n#[cfg(test)]\nmod other {\n    #[test]\n    fn concat()",
        );
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            ("src/lib.rs", &split),
            ("src/data/x.txt", "one\n"),
        ])
        .await;
        let r = dir.path();
        let before = text(r, "src/lib.rs::tests::reads").await;
        let hash = Include {
            path: "src/data/x.txt".parse().unwrap(),
            sha256: Sha256::digest(b"one\n").into(),
        }
        .hex();
        assert!(
            before.ends_with(&format!("\n\n@@ src/data/x.txt {hash}")),
            "{before}"
        );
        write(r, "src/data/x.txt", "two\n").await;
        assert_ne!(text(r, "src/lib.rs::tests::reads").await, before);
        let why = why(r, "src/lib.rs::other::concat").await;
        assert!(why.contains("reads a path that is no literal"), "{why}");
        // A file an include names that is not there cannot be told.
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
        tokio::fs::remove_file(r.join("src/data/x.txt"))
            .await
            .unwrap();
        let gone = why_in(
            &RustTestSpace::new(r.to_path_buf()),
            "src/lib.rs::tests::reads",
        )
        .await;
        assert!(
            gone.contains("src/data/x.txt, which an include there reads, is not there"),
            "{gone}"
        );
    }

    #[tokio::test]
    async fn a_module_that_cannot_be_read_cannot_be_told_for_the_tests_that_reach_it_only() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            (
                "src/lib.rs",
                "#[cfg(test)]\nmod fixtures_tests;\n#[cfg(test)]\nmod gone_tests;\n#[cfg(test)]\n#[cfg_attr(unix, path = \"p.rs\")]\nmod pathed_tests;\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn gone() { crate::fixtures_tests::seed(); crate::gone_tests::x(); }\n    }\n#[cfg(test)]\nmod pathed {\n    #[test]\n    fn pathed() { crate::pathed_tests::x(); }\n}\n#[cfg(test)]\nmod fine {\n    #[test]\n    fn fine() {}\n}\n",
            ),
            ("src/fixtures_tests.rs", FIXTURES),
        ])
        .await;
        let r = dir.path();
        let gone = why(r, "src/lib.rs::tests::gone").await;
        assert!(
            gone.contains("at src/lib.rs: the module `gone_tests` declared there cannot be read: neither src/gone_tests.rs nor src/gone_tests/mod.rs is there"),
            "{gone}"
        );
        let pathed = why(r, "src/lib.rs::pathed::pathed").await;
        assert!(pathed.contains("`#[path]` is not one literal"), "{pathed}");
        assert!(held(&text(r, "src/lib.rs::fine::fine").await).is_empty());
        // Where it cannot be told, the test lives nowhere, the files told named.
        let space = RustTestSpace::new(r.to_path_buf());
        let names = BTreeSet::from([n("src/lib.rs::tests::gone"), n("src/lib.rs::fine::fine")]);
        let placed = space.places(Side::After, &names).await.unwrap();
        match &placed[&n("src/lib.rs::tests::gone")] {
            Placement::Unplaced(why) => assert!(
                why.as_str()
                    .ends_with("the files told are src/fixtures_tests.rs, src/lib.rs"),
                "{why}"
            ),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            placed[&n("src/lib.rs::fine::fine")],
            Placement::Only(NonEmptySet::new("src/lib.rs".parse().unwrap(), []))
        );
        // A tree past its bounds: no test of the crate can be told.
        let small = RustTestSpace::new(r.to_path_buf()).with_tree_bounds(1, u64::MAX);
        let cut = why_in(&small, "src/lib.rs::fine::fine").await;
        assert!(cut.contains("are not read further"), "{cut}");
        let small = RustTestSpace::new(r.to_path_buf()).with_tree_bounds(usize::MAX, 10);
        assert!(
            why_in(&small, "src/lib.rs::fine::fine")
                .await
                .contains("bytes of them")
        );
    }

    /// Behavioural: there is no clean way to count reads, so this checks that one space
    /// answering two names, and a name again, gives the texts fresh spaces give, and
    /// that it keeps the trees it first read (a file edited after is not seen by it).
    #[tokio::test]
    async fn a_space_reads_the_trees_once_and_answers_as_fresh_spaces_do() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            ("src/lib.rs", LIB),
            ("src/fixtures_tests.rs", FIXTURES),
        ])
        .await;
        let r = dir.path();
        let names = [
            "src/lib.rs::tests::names_the_helper",
            "src/lib.rs::tests::names_nothing",
        ];
        let shared = RustTestSpace::new(r.to_path_buf());
        for name in names {
            let fresh = text(r, name).await;
            let got = shared.current(&n(name)).await.unwrap().unwrap();
            assert_eq!(String::from_utf8(got).unwrap(), fresh, "{name}");
        }
        // The trees were read with the first answer: an edit of the helper after is
        // not in them, so the same space tells the old text, a new space the new.
        write(r, "src/fixtures_tests.rs", "pub fn seed() -> u8 { 2 }\n").await;
        let old = shared.current(&n(names[0])).await.unwrap().unwrap();
        assert!(String::from_utf8(old).unwrap().contains("{ 1 }"));
        assert!(text(r, names[0]).await.contains("{ 2 }"));
    }

    /// The modules held by the test `name` of the package `demo` made of `files`.
    async fn held_in(files: &[(&str, &str)], name: &str) -> Vec<String> {
        let mut all = vec![("Cargo.toml", PACKAGE)];
        all.extend_from_slice(files);
        let dir = project(&all).await;
        held(&text(dir.path(), name).await)
    }

    /// Why the test `name` of the package `demo` made of `files` cannot be told.
    async fn why_in_files(files: &[(&str, &str)], name: &str) -> String {
        let mut all = vec![("Cargo.toml", PACKAGE)];
        all.extend_from_slice(files);
        let dir = project(&all).await;
        why(dir.path(), name).await
    }

    const SEED: &str = "pub fn seed() -> u8 { 1 }\n";

    #[tokio::test]
    async fn a_local_or_a_parameter_named_like_a_module_of_tests_holds_nothing() {
        let files = [
            (
                "src/lib.rs",
                "#[cfg(test)]\nmod tests;\n#[cfg(test)]\nmod checks {\n    fn approved(tests: &[u8]) -> usize { tests.len() }\n    #[test]\n    fn t() { let tests = [1u8]; approved(&tests); }\n}\n",
            ),
            ("src/tests.rs", SEED),
        ];
        assert!(held_in(&files, "src/lib.rs::checks::t").await.is_empty());
    }

    #[tokio::test]
    async fn a_path_by_super_or_crate_holds_the_module_it_names_and_not_one_of_the_same_name() {
        let b = "#[cfg(test)]\nmod fake_tests;\n#[cfg(test)]\nmod tests;\n";
        // `super::fake_tests::x` from `a::tests_in`, and `self::super::` as it, hold the
        // module of `a`, not that of `b`.
        for call in ["super::fake_tests::x()", "self::super::fake_tests::x()"] {
            let a = format!(
                "#[cfg(test)]\nmod fake_tests;\n#[cfg(test)]\nmod tests_in {{\n    #[test]\n    fn t() {{ {call}; }}\n}}\n"
            );
            let files = [
                ("src/lib.rs", "mod a;\nmod b;\n"),
                ("src/a.rs", a.as_str()),
                ("src/a/fake_tests.rs", SEED),
                ("src/b.rs", b),
                ("src/b/fake_tests.rs", SEED),
            ];
            assert_eq!(
                held_in(&files, "src/a.rs::tests_in::t").await,
                ["src/a/fake_tests.rs crate::a::fake_tests"],
                "{call}"
            );
        }
        // `crate::a::tests::x` holds the `tests` of `a` only.
        let files = [
            (
                "src/lib.rs",
                "mod a;\nmod b;\n#[cfg(test)]\nmod checks {\n    #[test]\n    fn t() { crate::a::tests::x(); }\n}\n",
            ),
            ("src/a.rs", "#[cfg(test)]\nmod tests;\n"),
            ("src/a/tests.rs", SEED),
            ("src/b.rs", b),
            ("src/b/tests.rs", SEED),
            ("src/b/fake_tests.rs", SEED),
        ];
        assert_eq!(
            held_in(&files, "src/lib.rs::checks::t").await,
            ["src/a/tests.rs crate::a::tests"]
        );
    }

    #[tokio::test]
    async fn self_and_a_child_name_the_module_beside_the_path() {
        let files = [
            ("src/lib.rs", "#[cfg(test)]\nmod tests;\n"),
            (
                "src/tests.rs",
                "mod x;\n#[test]\nfn via_self() { self::x::y(); }\n#[test]\nfn via_child() { x::y(); }\n#[test]\nfn via_neither() { y(); }\n",
            ),
            ("src/tests/x.rs", "pub fn y() {}\n"),
        ];
        for name in ["via_self", "via_child"] {
            assert_eq!(
                held_in(&files, &format!("src/tests.rs::{name}")).await,
                ["src/tests/x.rs crate::tests::x"],
                "{name}"
            );
        }
        assert!(
            held_in(&files, "src/tests.rs::via_neither")
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_use_tree_names_the_module_of_each_leaf_and_an_alias_of_it_is_followed() {
        let fx = ("src/fx.rs", "pub fn a() {}\npub fn b() {}\n");
        let lib = |part: &str| {
            format!(
                "#[cfg(test)]\nmod fx;\n#[cfg(test)]\nmod fixtures_tests;\n#[cfg(test)]\nmod tests {{\n{part}\n}}\n"
            )
        };
        let seed = ("src/fixtures_tests.rs", SEED);
        for (part, held_now) in [
            (
                "use super::fx::{a, b as c};\n#[test]\nfn t() { a(); c(); }",
                vec!["src/fx.rs crate::fx"],
            ),
            (
                "use crate::fixtures_tests as sd;\n#[test]\nfn t() { sd::seed(); }",
                vec!["src/fixtures_tests.rs crate::fixtures_tests"],
            ),
            (
                "use super::*;\n#[test]\nfn t() { fixtures_tests::seed(); }",
                vec!["src/fixtures_tests.rs crate::fixtures_tests"],
            ),
        ] {
            let lib = lib(part);
            let files = [("src/lib.rs", lib.as_str()), fx, seed];
            assert_eq!(
                held_in(&files, "src/lib.rs::tests::t").await,
                held_now,
                "{part}"
            );
        }
    }

    #[tokio::test]
    async fn a_path_of_another_crate_or_above_the_root_names_nothing_and_self_is_no_anchor() {
        let lib = |test: &str| {
            format!(
                "#[cfg(test)]\nmod other;\n#[cfg(test)]\nmod fx_tests;\n#[cfg(test)]\nmod x;\n#[cfg(test)]\nmod checks {{\n    struct S;\n    impl S {{\n        fn run() {{ {test} }}\n    }}\n    #[test]\n    fn t() {{ S::run(); }}\n}}\n"
            )
        };
        let helpers = [
            ("src/other.rs", SEED),
            ("src/fx_tests.rs", SEED),
            ("src/x.rs", SEED),
        ];
        for test in [
            "::other::seed();",
            "super::super::fx_tests::seed();",
            "Self::x();",
        ] {
            let lib = lib(test);
            let mut files = vec![("src/lib.rs", lib.as_str())];
            files.extend(helpers);
            assert!(
                held_in(&files, "src/lib.rs::checks::t").await.is_empty(),
                "{test}"
            );
        }
        // The same, from the top of the file: `super` above the root.
        let files = [
            (
                "src/lib.rs",
                "#[cfg(test)]\nmod fx_tests;\n#[test]\nfn t() { super::fx_tests::seed(); }\n",
            ),
            ("src/fx_tests.rs", SEED),
        ];
        assert!(held_in(&files, "src/lib.rs::t").await.is_empty());
    }

    #[tokio::test]
    async fn a_path_is_read_from_the_inline_module_it_is_written_in() {
        let files = |a: &'static str| {
            vec![
                ("src/lib.rs", "mod a;\nmod b;\n"),
                ("src/a.rs", a),
                ("src/a/x_tests.rs", SEED),
                ("src/b.rs", "#[cfg(test)]\nmod x_tests;\n"),
                ("src/b/x_tests.rs", SEED),
            ]
        };
        // `super::super` from `tests::inner` is `a`, where `x_tests` is; from `tests`
        // it would be the root, and `x_tests` an unresolved head of both modules.
        let a = "#[cfg(test)]\nmod x_tests;\n#[cfg(test)]\nmod tests {\n    mod inner {\n        use super::super::x_tests::f;\n    }\n    #[test]\n    fn t() {}\n}\n";
        assert_eq!(
            held_in(&files(a), "src/a.rs::tests::t").await,
            ["src/a/x_tests.rs crate::a::x_tests"]
        );
    }

    #[tokio::test]
    async fn a_module_that_cannot_be_read_is_reached_by_a_path_or_a_head_of_it() {
        let lib = |part: &str| {
            format!(
                "mod a;\n#[cfg(test)]\nmod missing_tests;\n#[cfg(test)]\nmod tests {{\n{part}\n    #[test]\n    fn other() {{}}\n}}\n"
            )
        };
        let a = ("src/a.rs", "#[cfg(test)]\nmod missing_tests;\n");
        for part in [
            "#[test]\nfn t() { crate::a::missing_tests::f(); }",
            "use super::*;\n#[test]\nfn t() { missing_tests::f(); }",
        ] {
            let lib = lib(part);
            let files = [("src/lib.rs", lib.as_str()), a];
            let said = why_in_files(&files, "src/lib.rs::tests::t").await;
            assert!(
                said.contains("the module `missing_tests` declared there cannot be read"),
                "{part}: {said}"
            );
        }
        // A test that names no path to it can be told.
        let lib = lib("");
        let files = [("src/lib.rs", lib.as_str()), a];
        assert!(held_in(&files, "src/lib.rs::tests::other").await.is_empty());
    }

    #[tokio::test]
    async fn an_alias_of_a_module_of_code_keeps_the_names_after_it() {
        // An alias of the root in the module of code around the tests, then a path by
        // `super` that goes through it.
        let files = [
            ("src/lib.rs", "#[cfg(test)]\nmod fx_tests;\nmod a;\n"),
            ("src/a.rs", "mod b;\n"),
            (
                "src/a/b.rs",
                "use super::super as shelf;\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { super::shelf::fx_tests::seed(); }\n}\n",
            ),
            ("src/fx_tests.rs", SEED),
        ];
        assert_eq!(
            held_in(&files, "src/a/b.rs::tests::t").await,
            ["src/fx_tests.rs crate::fx_tests"]
        );
        // An alias of an anchor, then the module.
        let files = [
            ("src/lib.rs", "#[cfg(test)]\nmod fx_tests;\nmod a;\n"),
            (
                "src/a.rs",
                "#[cfg(test)]\nmod tests {\n    use super::super as up;\n    #[test]\n    fn t() { up::fx_tests::seed(); }\n}\n",
            ),
            ("src/fx_tests.rs", SEED),
        ];
        assert_eq!(
            held_in(&files, "src/a.rs::tests::t").await,
            ["src/fx_tests.rs crate::fx_tests"]
        );
    }

    #[tokio::test]
    async fn a_macro_resolves_where_it_is_called_but_for_its_crate() {
        let lib = |part: &str| {
            format!(
                "#[cfg(test)]\nmod fx_tests;\nmod a;\nmod b;\n#[cfg(test)]\nmod tests {{\n{part}\n}}\n"
            )
        };
        let helpers = [
            ("src/fx_tests.rs", SEED),
            ("src/a.rs", "#[cfg(test)]\nmod fx;\n"),
            ("src/a/fx.rs", SEED),
            ("src/b.rs", "#[cfg(test)]\nmod fx;\n"),
            ("src/b/fx.rs", SEED),
        ];
        for (part, held_now) in [
            // Its body is read at the call: every segment is a head.
            (
                "macro_rules! seeded { () => { super::fx_tests::seed() } }\n#[test]\nfn t() { seeded!(); }",
                vec!["src/fx_tests.rs crate::fx_tests"],
            ),
            (
                "macro_rules! seeded { () => { super::super::fx_tests::seed() } }\n#[test]\nfn t() { seeded!(); }",
                vec!["src/fx_tests.rs crate::fx_tests"],
            ),
            // `$crate` is the crate, so the path is resolved from its root.
            (
                "macro_rules! seeded { () => { $crate::a::fx::seed() } }\n#[test]\nfn t() { seeded!(); }",
                vec!["src/a/fx.rs crate::a::fx"],
            ),
            // A lone identifier in the arguments of a macro of another crate.
            (
                "#[test]\nfn t() { suite!(fx_tests); }",
                vec!["src/fx_tests.rs crate::fx_tests"],
            ),
            // A lone identifier in the arguments of a macro of the standard library or
            // a logging one is a local.
            (
                "#[test]\nfn t() { let fx_tests = vec![1]; assert_eq!(fx_tests.len(), 1); }",
                vec![],
            ),
            (
                "#[test]\nfn t() { let fx_tests = 1; info!(n = fx_tests.len()); }",
                vec![],
            ),
        ] {
            let lib = lib(part);
            let mut files = vec![("src/lib.rs", lib.as_str())];
            files.extend(helpers);
            assert_eq!(
                held_in(&files, "src/lib.rs::tests::t").await,
                held_now,
                "{part}"
            );
        }
    }

    #[test]
    fn a_path_is_a_run_of_segments_or_a_leaf_of_a_use_tree() {
        type Found = BTreeSet<(Anchor, Vec<String>, Site, Shape)>;
        let paths = |text: &str| -> Found {
            let mut refs = Refs::default();
            refs.add(text.parse().unwrap());
            refs.paths
                .into_iter()
                .map(|p| (p.anchor, p.segments, p.site, p.shape))
                .collect()
        };
        let s = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            paths("use a::{b, c::d as e, f::*, g::{self}}; use x; use ::y::z;"),
            Found::from([
                (Anchor::Plain, s(&["a", "b"]), Site::Code, Shape::UseLeaf),
                (
                    Anchor::Plain,
                    s(&["a", "c", "d"]),
                    Site::Code,
                    Shape::UseLeaf
                ),
                (Anchor::Plain, s(&["a", "f"]), Site::Code, Shape::UseLeaf),
                (Anchor::Plain, s(&["a", "g"]), Site::Code, Shape::UseLeaf),
                (Anchor::Plain, s(&["x"]), Site::Code, Shape::UseLeaf),
                (Anchor::Global, s(&["y", "z"]), Site::Code, Shape::UseLeaf),
            ])
        );
        // A lone identifier, a field and a path after a type are no path.
        assert_eq!(
            paths("self::super::a::f(); crate::b(); Vec::<u8>::new(); x.tests; let t = tests;"),
            Found::from([
                (Anchor::Super(1), s(&["a", "f"]), Site::Code, Shape::Path),
                (Anchor::Crate, s(&["b"]), Site::Code, Shape::Path),
            ])
        );
    }

    #[tokio::test]
    async fn a_file_in_no_crate_read_cannot_be_told_while_something_was_not_read() {
        let files = [
            ("src/lib.rs", "pub fn x() {}\n"),
            (
                "other/x.rs",
                "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
        ];
        let dir = project(&files).await;
        let r = dir.path();
        let bare = text(r, "other/x.rs::tests::t").await;
        write(
            r,
            "Cargo.toml",
            &format!("[workspace]\nmembers = [\"c/**\"]\n{PACKAGE}"),
        )
        .await;
        let why = why(r, "other/x.rs::tests::t").await;
        assert!(
            why.contains("it is in no crate read") && why.contains("`c/**`"),
            "{why}"
        );
        // With all of it read, a file in no crate holds nothing, as before.
        write(r, "Cargo.toml", PACKAGE).await;
        assert_eq!(text(r, "other/x.rs::tests::t").await, bare);
    }

    #[tokio::test]
    async fn a_file_under_a_module_that_cannot_be_read_cannot_be_told() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            ("src/lib.rs", "mod a;\n"),
            // It does not lex, so the modules under it are of no tree.
            ("src/a.rs", "mod b;\nfn broken() { ( }\n"),
            (
                "src/a/b.rs",
                "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
        ])
        .await;
        let why = why(dir.path(), "src/a/b.rs::tests::t").await;
        assert!(
            why.contains("it is in no crate read") && why.contains("the module `a`"),
            "{why}"
        );
    }

    #[test]
    fn the_end_of_a_range_is_no_member_and_a_path_after_a_keyword_is_of_another_crate() {
        let mut refs = Refs::default();
        refs.add(
            "fn f() { for i in 0..count {} return ::helpers::x(); }"
                .parse()
                .unwrap(),
        );
        assert!(!refs.members.contains("count"), "{:?}", refs.members);
        assert!(
            refs.paths
                .iter()
                .any(|p| p.anchor == Anchor::Global && p.segments == ["helpers", "x"]),
            "{:?}",
            refs.paths
        );
    }

    #[tokio::test]
    async fn a_test_lives_in_its_file_and_in_the_files_it_holds() {
        let dir = project(&[
            ("Cargo.toml", PACKAGE),
            ("src/lib.rs", LIB),
            ("src/fixtures_tests.rs", FIXTURES),
        ])
        .await;
        let space = RustTestSpace::new(dir.path().to_path_buf());
        let names = BTreeSet::from([
            n("src/lib.rs::tests::names_the_helper"),
            n("src/lib.rs::tests::names_nothing"),
            n("src/lib.rs::tests::missing"),
        ]);
        let placed = space.places(Side::After, &names).await.unwrap();
        let only = |files: &[&str]| {
            Placement::Only(
                NonEmptySet::from_set(files.iter().map(|f| f.parse().unwrap()).collect()).unwrap(),
            )
        };
        let expected: BTreeMap<ArtifactName, Placement> = [
            (
                n("src/lib.rs::tests::names_the_helper"),
                only(&["src/fixtures_tests.rs", "src/lib.rs"]),
            ),
            (n("src/lib.rs::tests::names_nothing"), only(&["src/lib.rs"])),
            // A test not there lives where it would be.
            (n("src/lib.rs::tests::missing"), only(&["src/lib.rs"])),
        ]
        .into_iter()
        .collect();
        assert_eq!(placed, expected);
    }
}
