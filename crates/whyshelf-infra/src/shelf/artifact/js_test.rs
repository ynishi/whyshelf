//! The adapter of the space `js-test`: the tests of a project's JavaScript and
//! TypeScript source, by the file they are in and their titles, read from the
//! source without running any program.
//!
//! | what | what it does | with |
//! |---|---|---|
//! | [`JsTestSpace::resolve`] | reads each file named, by the rules of the space `file`, lexes it once with the lexer of this module, indexes its levels once, and finds each test named in the index; the text of a test call is what is compared | [`file`](mod@file) |
//! | [`JsTestSpace::current`] | the text of a named test now | the same |
//! | [`presets`] | the preset `js-test`: the space, `compare = "bytes"`, `means = "depends"` | values |
//!
//! ```text
//!   name ──<file>.ts::<title>::…::<title>?── no ──► not a name (why)
//!     │ yes
//!   the file, by the rules of the space `file`
//!     ├─ no file there ──► absent
//!     ├─ a link out of the root, too large, cannot be read, not UTF-8 ──► cannot tell
//!     └─ lexed once (this module's lexer, no recursion), nesting held to MAX_DEPTH
//!          ├─ does not lex, or nests too deep ──► cannot tell
//!          └─ indexed once: the top level, and the body of every `describe` with a
//!             literal title, each scanned once, their calls by title
//!               ├─ found: the test call's tokens, comments left out ──► there (its text)
//!               ├─ not found, and something on the way may make it ──► cannot tell
//!               └─ not found ──► absent
//! ```
//!
//! Resolving runs no program: it reads the file and lexes it. No test runner
//! (`vitest list`, `jest --listTests`, `playwright test --list`) is asked, since
//! running one runs the project's code and its configuration.
//!
//! # The names of the space
//!
//! A name is `<file>::<title>(::<title>)*`:
//!
//! - `<file>` is everything before the first `::` that no `\` escapes: a path of the
//!   space `file` (relative to the root, `/` between parts, no empty, `.` or `..`
//!   part), ending in one of [`EXTENSIONS`] (`.ts`, `.tsx`, `.mts`, `.cts`, `.js`,
//!   `.jsx`, `.mjs`, `.cjs`).
//! - Each title is a non-empty string, the parts being split at each `::` that no
//!   `\` escapes. In a title `\:` stands for `:` and `\\` for `\`; any other `\` is
//!   no name. So a title that holds `::` is written `a\:\:b`, and one that ends in
//!   `:` before the next title `a\:::b`; `a:::b` is the titles `a` and `:b`.
//! - The last title is a test's, the ones before it those of the `describe`s it is
//!   in, outermost first:
//!   `tests/cart.test.ts::Cart::adds an item`.
//!
//! A title is compared with the value of the literal in the source, code point by
//! code point, case included, not normalized. A name holds no control character,
//! so a test whose title holds one (a tab, a line break) cannot be named. Any other
//! text is not a name of the space, and the adapter says so with why.
//!
//! # Reading the file
//!
//! The file is read as the space `file` reads one ([`file`](mod@file)): its path is
//! compared as written, ASCII case included; a symbolic link on it is followed only
//! inside the root; and a file larger than [`MAX_FILE_BYTES`] (the largest file the
//! space `file` reads when it searches for moves) is not read. It is UTF-8, a mark
//! of byte order at its start skipped.
//!
//! It is lexed once, in one loop with no recursion, into a flat list of tokens in
//! which each bracket knows its partner; no expression is parsed, so no nesting of
//! them (a long chain of operators, of arrows, of awaits) can exhaust the stack.
//! The lexer reads:
//!
//! - a hashbang (`#!…`) on the first line, skipped;
//! - white space and line ends (`\n`, `\r`, U+2028, U+2029), which separate tokens;
//! - line comments (`//…`) and block comments (`/*…*/`), dropped;
//! - strings in `'` or `"`, with `\` escaping the next character (a line end, too);
//!   an unescaped line end in one is a fault;
//! - template literals in `` ` ``, with `\` escaping the next character, and
//!   substitutions `${ … }` whose code is lexed as code, nested to any depth up to
//!   the bound: the `}` that closes a substitution resumes the template;
//! - regular expression literals `/…/flags`, with `\` escapes and classes `[…]` (in
//!   which `/` closes nothing), on one line, told from division by the rule below;
//! - numbers (decimal with `.`, `e±`, `_` and `n`; `0x`, `0o`, `0b`);
//! - identifiers and keywords (ASCII letters, digits, `$`, `_`, `\u` escapes, and
//!   every character beyond ASCII that is not white space), and private names
//!   (`#x`);
//! - punctuators, the longest that fits (`>>>=`, `...`, `?.`, `=>`, …); `?.` before
//!   a digit is `?`;
//! - brackets `( )`, `[ ]`, `{ }`, each closed by its own kind;
//! - in a file that may hold JSX (`.tsx`, `.jsx`, `.js`, `.mjs`, `.cjs`; not `.ts`,
//!   `.mts`, `.cts`, where `<T>x` is a cast): an element. A `<` where an operand
//!   starts, before a name or `>`, opens a tag; in a tag, `/` is a punctuator,
//!   strings may span lines and hold no escapes, `{ … }` is code, and `>` ends the
//!   tag. After an opening tag that does not end in `/>` come its children: text up
//!   to the next `<` or `{` is one token (its white space at both ends left out),
//!   `{ … }` is code, `<…>` is a child element, and `</…>` closes the element. A
//!   tag in which a `,` or `extends` comes first is a list of type parameters
//!   (`<T,>(x: T) => x`), and no tag.
//!
//! The brackets, the substitutions of templates and the elements of JSX nest at
//! most [`MAX_DEPTH`] deep. Anything else (a character no token starts with, a
//! string, template, comment, expression or element not closed, a bracket closed by
//! another kind or closing nothing) is a fault, and the file cannot be told.
//!
//! ## A regular expression or a division
//!
//! A `/` that starts no comment starts a regular expression when the token before
//! it (comments do not count) cannot end an operand, and is division (`/` or `/=`)
//! when it can:
//!
//! | the token before `/` | `/` is |
//! |---|---|
//! | none (the start of the file) | a regular expression |
//! | `(`, `[`, `{`, or the `${` that opens a substitution | a regular expression |
//! | a punctuator but `++`, `--` and a postfix `!` (`=`, `,`, `(`, `&&`, `?`, `:`, `=>`, …) | a regular expression |
//! | `++`, `--` | division |
//! | `!` right after an identifier, a literal, `)` or `]`, with no space (`total!`, TypeScript's non-null) | division |
//! | a keyword that is followed by an operand (`return`, `typeof`, `instanceof`, `in`, `new`, `delete`, `void`, `throw`, `case`, `do`, `else`, `yield`, `await`; `of` only in the head of a `for`), not after `.` or `?.` | a regular expression |
//! | any other identifier or keyword (`this`, `x`, `a.return`, `of` elsewhere) | division |
//! | a number, a string, a template, a regular expression | division |
//! | `]` | division |
//! | `)` whose `(` follows `if`, `while`, `for`, `with` (or `for await`) | a regular expression |
//! | any other `)` | division |
//! | `}` whose `{` opens a block | a regular expression |
//! | `}` whose `{` opens an object literal | division |
//! | in a tag of JSX | a punctuator, never a regular expression |
//!
//! A `{` opens a block, not an object literal, when it is the body of a `class`,
//! `interface`, `enum` or `namespace` (the first `{` after the keyword, at its
//! depth), or when the token before it is none, `;`, `{`, `}`, `)`, `=>`, an
//! identifier that is not a keyword followed by an operand, or a literal; after
//! `(`, `[`, `,`, the `${` of a substitution, `]`, any other punctuator (`=`, `:`,
//! `?`, `&&`, …) or a keyword followed by an operand, it opens an object literal.
//!
//! # Finding the test
//!
//! The source is indexed level by level. A level is the top of the file, or the
//! brace body of the callback of a `describe` with a literal title. With it go the
//! blocks of its statements, read as part of the level: the braces after
//! `if (…)`, `for (…)`, `while (…)`, `with (…)`, `switch (…)`, `catch (…)`, `else`,
//! `do`, `try`, `finally`, the `:` of a `case …` or `default` and of a label
//! (`outer: { … }`), a `namespace X.Y`, `module X`, `declare module "x"` or
//! `declare global`, and a block that starts a statement. The body of a function
//! is never part of a level: a `function` on a level is stepped over, its name,
//! type parameters, parameters and return type (`): { done: boolean } {`) with it;
//! nor is an arrow's body, a class's, or an object literal.
//!
//! On a level, a test call is a callee, then its argument list `( … )`:
//!
//! - the callee is one of [`DESCRIBES`] (`describe`, `suite`, `context`,
//!   `xcontext`, `xdescribe`, `fdescribe`) or [`TESTS`] (`it`, `test`, `specify`,
//!   `xspecify`, `xit`, `fit`, `xtest`), not after `.`, `?.`, `function` or
//!   `class`;
//! - followed by any of the members [`MODIFIERS`] (`.only`, `.skip`, `.todo`,
//!   `.concurrent`, `.sequential`), for a test also [`TEST_MODIFIERS`] (`.fails`,
//!   `.failing`, `.fixme`, `.fail`, `.slow`), and `.skipIf(…)` or `.runIf(…)`
//!   applied, in any order: `it.concurrent.only(…)`, `test.skipIf(isCi)(…)`;
//! - Playwright's `test.describe` is a `describe`, with any of
//!   [`PLAYWRIGHT_DESCRIBE_MODIFIERS`] (`.only`, `.skip`, `.fixme`, `.serial`,
//!   `.parallel`): `test.describe.serial.only(…)`;
//! - then a list of type arguments `<…>` may come before the `(`:
//!   `it<Ctx>("title", …)`. One longer than [`MAX_TYPE_ARGUMENTS`] tokens is not
//!   stepped over: the call is not read, and may make a test (below).
//!
//! Its first argument (the tokens up to the first `,` of the list) is its title
//! when it is one string literal, or one template literal with no `${}`, decoded
//! to its value (escapes as JavaScript reads them; a template's line ends as
//! `\n`); any other first argument (a variable, a template with `${}`, a
//! concatenation, a class) is a title that is not literal, as is a literal whose
//! value is no Unicode text (a lone surrogate).
//!
//! A call of Playwright's `test` (the callee `test` itself, not `it` or another
//! alias) with `.skip`, `.fixme`, `.slow` or `.fail` whose first argument is not a
//! literal title is an annotation, and makes nothing, when it has no argument
//! (`test.slow()`), a callback first (`test.skip(({ browserName }) => …)`), or a
//! first argument that is no string or template followed by a literal description
//! and nothing more (`test.skip(isMobile, "why")`). Any other such call, one
//! argument that could be a title (`test.skip(name)`, `it.skip(name)`) among them,
//! is a test whose title is not literal: Vitest's `test.skip(name)` registers one.
//!
//! The children of a `describe` are the calls of the body of its callback: the first
//! argument after the title that is a function (`function (…) { … }`, with a name,
//! `*`, type parameters or a return type) or an arrow (`… => { … }`) whose body is a
//! brace group. Each `describe` of the title on the level is searched (twins under
//! two `if`s, say), and two tests of the name on the levels searched are one, their
//! texts joined by a line end in the order of the file.
//!
//! Each level is scanned once, and its `describe`s and tests are kept by title, so
//! finding many names in one file costs the length of the file once, then the
//! length of each name's way.
//!
//! # What may make a test
//!
//! When a test is not found, the answer is "cannot tell" rather than "absent" if a
//! level searched holds (each looked at only on the levels where the test, or the
//! `describe` sought there, would be):
//!
//! - a call of the role sought (a `describe` on a level where a `describe` is
//!   sought, a test where the test is) whose title is not literal;
//! - a `.each` of that role (`describe.each`, `it.each`, `test.each`);
//! - a `describe` of the title sought whose callback is not a function with a brace
//!   body (`describe('x', makeCases)`);
//! - a call of the role whose type arguments are too long to be stepped over;
//! - a call of another function that starts a statement of the level: a statement
//!   that begins with a name, `[…]` or `(…)`, after `await` or `void` if any, and
//!   is a chain of `.name`, `?.name`, `[…]`, `(…)` and tagged templates holding a
//!   call (`cases.forEach(…)`, `[1, 2].map(…)`, `sharedBehaviour()`), but where the
//!   chain starts with one of [`HARMLESS`] (the hooks `beforeEach`, `afterAll`, …,
//!   and `vi`, `jest`, `expect`, `console`, `require`), or with `test.` and one of
//!   [`PLAYWRIGHT_HARMLESS`] (`test.use`, `test.beforeEach`, `test.setTimeout`,
//!   `test.slow()`, …) or `test.describe.configure`, which make no test.
//!
//! A statement starts at the start of the level or of a block, after `;`, `}`,
//! `else`, `do`, the `)` of the head of `if`, `for`, `while` or `with`, the `:` of a
//! case or a label, and after a line end that follows a token that can end an
//! operand (the rule that inserts `;`). A call inside an expression
//! (`const server = setupServer()`), inside a test's body, a hook's, or a helper
//! function's, does not count: a helper that makes tests is seen where it is called
//! on the level, not where it is defined.
//!
//! The reason names the first [`SHOWN`] that may make the test, in the order of the
//! way, each with its line and column, and how many more there are: every call or
//! title seen is one, and each is counted once.
//!
//! So where a level on the way calls a helper or has a title that is not literal, a
//! test removed (or renamed) reads as "cannot tell", not as gone: an approval of it
//! stays fresh, with a notice that it cannot be told.
//!
//! # What is compared
//!
//! The text of a test is the test call's own tokens, from its callee to the `)`
//! that closes its arguments (`test.skipIf(a)("t", () => { … })` whole), each as the
//! source writes it, comments left out, one space between two tokens: the contents
//! of strings, templates and regular expressions are kept exactly. A line end
//! between two tokens is kept, as a line end in place of the space, where it
//! changes what the code does (where JavaScript inserts `;` whatever comes next):
//! after `return`, `throw`, `break`, `continue`, `yield` and `async`, and before
//! `++` and `--`. Elsewhere a line end is white space: an edit that only breaks a
//! line or joins two (a formatter's) makes nothing stale.
//!
//! So an edit of white space or comments, or of another test, makes nothing stale,
//! and any other edit of the call (its title, its modifiers, its body) does. The
//! text is what [`ArtifactReading::of_bytes`] hashes, so every comparison works
//! over it:
//!
//! - `bytes` (the preset `js-test`): every token.
//! - `lines`: the lines of the text (the line ends kept above, and those inside a
//!   template), so it tells what `bytes` tells, under another hash.
//! - `presence`: only that the test is there.
//!
//! There is no address: the text is made, not stored, so nothing is read back.
//!
//! # What it costs
//!
//! A token is 12 bytes, and a file of `n` bytes has at most `n` tokens (one a byte,
//! as in `f();f();…`), so lexing holds the file and at most `12 n` bytes of tokens,
//! and up to half as much again while the list of tokens grows. A level holds its
//! calls by title and no more than [`SHOWN`] of what may make a test, with a count.
//! Measured (`whyshelf check`, built for release, the most resident memory) on a
//! file of 16 MiB, the largest a reading takes: 296 MB for `f();` repeated (18
//! times the file), 293 MB for `{}` repeated, 193 MB for `x` on 8 Mi lines.
//!
//! # The answers
//!
//! | answer | when |
//! |---|---|
//! | there | the test is found, with its text |
//! | absent | no file is there; or it is scanned, and has no `describe` or test of the titles on the way, and nothing on the way may make one |
//! | cannot tell | a symbolic link on the path leaves the root or is broken; the file cannot be read, is too large, is not UTF-8, does not lex, or nests too deep; or the test is not found, and something on the way may make it |
//! | not a name | the text is not of the grammar above |
//!
//! # Known limits
//!
//! - The text is the test call alone. An edit of a helper it calls, of a hook
//!   (`beforeEach`), of an `import`, of a fixture it reads, or of the `describe`
//!   around it outside the test, leaves an approval of it fresh.
//! - Titles are read only as literals: a test whose title is a variable, a
//!   template with `${}`, or comes from `.each`, cannot be named; when one is on the
//!   way, a test not found cannot be told.
//! - Tests a helper makes are not followed into the helper: a test not found where
//!   a helper is called on the way cannot be told. A helper called inside an
//!   expression (`const t = makeTests()`) or after `export default` is not seen,
//!   and a test it makes reads as absent; so does one made by a callee this adapter
//!   does not know (`test.extend`'s result, say) when it is not called where a
//!   statement starts.
//! - The file is lexed, not parsed: one that lexes but is no valid program is read
//!   as far as its calls can be told. A `/` after the `}` of a function expression
//!   (`f = function () {} / 2`) is read as a regular expression. In JSX, the white
//!   space inside a text (not at its ends) is kept as written, so re-indenting a
//!   text of several lines makes the call that holds it stale. In a file that may
//!   hold JSX, a generic function type after an operator (`type F = <T>(x: T) =>
//!   T`) is read as a tag, and the file may not lex (cannot tell).
//! - A list of type arguments longer than [`MAX_TYPE_ARGUMENTS`] tokens before the
//!   `(` of a test call is not stepped over: the call is not read, and a test not
//!   found where one is on the way cannot be told.
//! - The file is read whether or not the runner runs it: a test in a file no
//!   configuration includes, or under a condition never met, reads as there.
//! - Moves are not suggested ([`MoveSource`](whyshelf_core::app::shelf::MoveSource)).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::PathBuf;

use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, Comparison, Space, SpaceAsks, SpaceReadings,
};
use whyshelf_core::domain::shelf::schema::{BuiltinPresets, PresetName, PresetValues, Propagation};

use super::file::{self, Bounded};

/// The name of the space this adapter resolves names in.
pub const JS_TEST_SPACE: &str = "js-test";

/// The name of the preset this adapter offers.
pub const JS_TEST_PRESET: &str = "js-test";

/// The most bytes of a file read: the largest file the space `file` reads when it
/// searches for moves ([`ScanLimits::file_bytes`](super::file::ScanLimits::file_bytes));
/// a larger one cannot be told.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The deepest nesting of brackets, substitutions of templates and elements of JSX
/// a file is lexed with; a deeper one cannot be told. Nothing recurses on the
/// nesting, so the bound is not for the stack but for the reader: code people
/// write nests far less, and a file that nests deeper is more likely made than
/// written.
pub const MAX_DEPTH: usize = 128;

/// The most tokens (a group being one) of a list of type arguments stepped over
/// between a test call's callee and its `(`: a longer one is no test call. The
/// bound keeps a file of many `<` from costing the square of its length.
pub const MAX_TYPE_ARGUMENTS: usize = 256;

/// How many of what may make a test a reason names; the rest are counted.
pub const SHOWN: usize = 5;

/// The endings of the path of a file a name may name.
pub const EXTENSIONS: &[&str] = &[".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"];

/// The endings of the files that may not hold JSX: TypeScript's, where `<T>x` is a
/// cast.
const NO_JSX: &[&str] = &[".ts", ".mts", ".cts"];

/// The callees of a `describe`: its aliases in Mocha, Jasmine and Jest.
pub const DESCRIBES: &[&str] = &[
    "describe",
    "suite",
    "context",
    "xcontext",
    "xdescribe",
    "fdescribe",
];

/// The callees of a test: its aliases in Mocha, Jasmine and Jest.
pub const TESTS: &[&str] = &["it", "test", "specify", "xspecify", "xit", "fit", "xtest"];

/// The members a test call's callee may have, which change how the test or the
/// `describe` runs and not what it is.
pub const MODIFIERS: &[&str] = &["only", "skip", "todo", "concurrent", "sequential"];

/// The members a test's callee may have beyond [`MODIFIERS`]: Vitest's `fails`,
/// Jest's `failing`, and Playwright's `fixme`, `fail` and `slow`.
pub const TEST_MODIFIERS: &[&str] = &["fails", "failing", "fixme", "fail", "slow"];

/// The members Playwright's `test.describe` may have.
pub const PLAYWRIGHT_DESCRIBE_MODIFIERS: &[&str] = &["only", "skip", "fixme", "serial", "parallel"];

/// The members of `test` that, called where a statement starts, make no test in
/// Playwright: its hooks, its settings, and its annotations.
pub const PLAYWRIGHT_HARMLESS: &[&str] = &[
    "use",
    "beforeEach",
    "afterEach",
    "beforeAll",
    "afterAll",
    "setTimeout",
    "slow",
    "fixme",
    "skip",
    "fail",
    "info",
];

/// The members that, on a test call with a title that is not literal, may make it
/// an annotation of Playwright.
const ANNOTATIONS: &[&str] = &["skip", "fixme", "slow", "fail"];

/// The first names of the calls that start a statement of a level and make no test:
/// the hooks of the runners, their objects of mocks and assertions, and loading a
/// module.
pub const HARMLESS: &[&str] = &[
    "beforeEach",
    "afterEach",
    "beforeAll",
    "afterAll",
    "before",
    "after",
    "setup",
    "teardown",
    "suiteSetup",
    "suiteTeardown",
    "vi",
    "jest",
    "expect",
    "console",
    "require",
];

/// The keywords after which an operand starts: a `/` after one starts a regular
/// expression, and a `{` an object literal (`of` only in the head of a `for`).
const OPERAND_KEYWORDS: &[&str] = &[
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "yield",
    "await",
];

/// The keywords after which a line end ends the statement whatever comes next.
const RESTRICTED: &[&str] = &["return", "throw", "break", "continue", "yield", "async"];

/// The keywords whose first `{` (at their depth) is a body, a block.
const BODY_KEYWORDS: &[&str] = &["class", "interface", "enum", "namespace"];

/// The words that are never the first name of a call of a helper (a statement that
/// starts with one is no call: `if (…)`, `import(…)`, `new X()`).
const KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "namespace",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// The presets this adapter offers when it resolves the names of `space`: one,
/// [`JS_TEST_PRESET`], for a link to the tests that show a document holds, that
/// stands on each token of them (`space` that space, `compare = "bytes"`,
/// `means = "depends"`): an edit of a test makes the approvals that name it stale,
/// one of another test in the file does not, and a test that goes away breaks them.
pub fn presets(space: &Space) -> BuiltinPresets {
    let values = PresetValues {
        space: Some(space.as_str().to_owned()),
        compare: Some("bytes".to_owned()),
        ignore: None,
        means: Some(Propagation::Depends),
    };
    match PresetName::new(JS_TEST_PRESET) {
        Ok(name) => BuiltinPresets::from([(name, values)]),
        // Not a name: no preset. The test of this module holds it to be one.
        Err(_) => BuiltinPresets::new(),
    }
}

/// Why the tests could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum JsTestError {
    /// The domain refused a reason this adapter gave (a bug if it ever happens: the
    /// reason is never blank).
    #[error("the domain refused a reason of the adapter")]
    Reason(#[source] ValueError),
}

/// Why a text is no name of the space.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
enum NotAName {
    #[error("no `::` follows the path of the file, so no test is named")]
    NoTitle,
    #[error(
        "the path `{0}` does not end in `.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs` or `.cjs`"
    )]
    NotScript(String),
    #[error("`{path}` is no path under the root of the project: {why}")]
    NoPath { path: String, why: String },
    #[error("a title between `::` is empty")]
    EmptyTitle,
    #[error("a title holds `{0}`, and only `\\:` and `\\\\` are escapes in a title")]
    BadEscape(String),
}

impl NotAName {
    /// The reason a reading gives.
    fn reason(&self) -> String {
        format!("no test name (`<file>.ts::<describe>::…::<test>`): {self}")
    }
}

/// Why a test cannot be told.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
enum Untellable {
    #[error(
        "a symbolic link on the path of {0} leads outside the root of the project, or is broken: nothing is read through it"
    )]
    Outside(RelPath),
    #[error("{path} is larger than {max} bytes, and is not read")]
    TooLarge { path: RelPath, max: u64 },
    #[error("{path} cannot be read: {why}")]
    Unreadable { path: RelPath, why: String },
    #[error("{0} is not UTF-8, so it is no JavaScript or TypeScript source")]
    NotUtf8(RelPath),
    #[error("{path} does not lex as JavaScript or TypeScript: {fault}{at}")]
    Lex {
        path: RelPath,
        fault: LexFault,
        at: Place,
    },
    #[error(
        "{path} nests brackets, substitutions of templates or elements deeper than {max}, and is not read further"
    )]
    TooDeep { path: RelPath, max: usize },
    #[error(
        "{path} has no such test as written, but these may make one, and no program is run: {by}"
    )]
    MayBeMade { path: RelPath, by: String },
    #[error("the reading of {path} stopped: {why}")]
    Stopped { path: RelPath, why: String },
}

/// A line and a column (in characters) of a file, from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Place {
    line: usize,
    column: usize,
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, " (at line {}, column {})", self.line, self.column)
    }
}

/// The tests of the JavaScript and TypeScript source of a project.
#[derive(Debug, Clone)]
pub struct JsTestSpace {
    root: PathBuf,
    /// The most bytes of a file read.
    max_file: u64,
}

impl JsTestSpace {
    /// The tests in the files under `root`.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            max_file: MAX_FILE_BYTES,
        }
    }

    /// For each name: not a name, the hash of the test's text under each comparison
    /// asked, absent, or cannot tell (with why). Every name asked about has an
    /// answer; each file is read, lexed and indexed once.
    pub async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, JsTestError> {
        let mut readings = SpaceReadings::new();
        let mut by_file: BTreeMap<RelPath, Vec<Asked<'_>>> = BTreeMap::new();
        for (name, comparisons) in asks {
            match TestName::parse(name.as_str()) {
                Ok(test) => {
                    by_file
                        .entry(test.file.clone())
                        .or_default()
                        .push((name, comparisons, test))
                }
                Err(why) => {
                    readings.insert(
                        name.clone(),
                        ArtifactReading::not_a_name(reason(why.reason())?),
                    );
                }
            }
        }
        for (path, tests) in by_file {
            let wanted: Vec<Wanted> = tests.iter().map(|(_, _, t)| t.wanted.clone()).collect();
            let found = self.find_in(&path, wanted).await;
            for ((name, comparisons, _), found) in tests.into_iter().zip(found) {
                let reading = match found {
                    Found::Text(text) => {
                        ArtifactReading::of_bytes(text.as_bytes(), comparisons, None)
                    }
                    Found::Absent => ArtifactReading::absent(),
                    Found::CannotTell(why) => {
                        ArtifactReading::cannot_tell(reason(why.to_string())?)
                    }
                };
                readings.insert(name.clone(), reading);
            }
        }
        Ok(readings)
    }

    /// The text of a test now (what is compared), or none when the name is none, or
    /// the test is not there or cannot be told.
    pub async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, JsTestError> {
        let Ok(test) = TestName::parse(name.as_str()) else {
            return Ok(None);
        };
        let found = self.find_in(&test.file, vec![test.wanted]).await;
        Ok(match found.into_iter().next() {
            Some(Found::Text(text)) => Some(text.into_bytes()),
            Some(Found::Absent | Found::CannotTell(_)) | None => None,
        })
    }

    /// What each of `wanted` comes to in the file at `path`, in their order.
    async fn find_in(&self, path: &RelPath, wanted: Vec<Wanted>) -> Vec<Found> {
        let every = |found: Found| vec![found; wanted.len()];
        let untold = |why: Untellable| every(Found::CannotTell(why));
        let bytes = match file::read_within(&self.root, path, self.max_file).await {
            Ok(Bounded::Bytes(bytes)) => bytes,
            Ok(Bounded::NoFile) => return every(Found::Absent),
            Ok(Bounded::Outside) => return untold(Untellable::Outside(path.clone())),
            Ok(Bounded::TooLarge) => {
                return untold(Untellable::TooLarge {
                    path: path.clone(),
                    max: self.max_file,
                });
            }
            Err(e) => {
                return untold(Untellable::Unreadable {
                    path: path.clone(),
                    why: e.to_string(),
                });
            }
        };
        let count = wanted.len();
        let at = path.clone();
        match tokio::task::spawn_blocking(move || look_up(&at, bytes, &wanted)).await {
            Ok(found) => found,
            Err(e) => vec![
                Found::CannotTell(Untellable::Stopped {
                    path: path.clone(),
                    why: e.to_string(),
                });
                count
            ],
        }
    }
}

/// The domain's reason of a text this adapter wrote.
fn reason(why: String) -> Result<NonBlank, JsTestError> {
    NonBlank::new(why).map_err(JsTestError::Reason)
}

/// A name of the space, read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TestName {
    file: RelPath,
    wanted: Wanted,
}

/// What is looked for in one file: the titles of the `describe`s, outermost first,
/// and the title of the test.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Wanted {
    describes: Vec<String>,
    test: String,
}

/// A name asked about, the comparisons asked of it, and the name read.
type Asked<'a> = (&'a ArtifactName, &'a BTreeSet<Comparison>, TestName);

impl TestName {
    /// The one reader of the grammar of the names (on the page of the module).
    fn parse(text: &str) -> Result<Self, NotAName> {
        let parts = split_titles(text);
        let Some((file, titles)) = parts.split_first() else {
            return Err(NotAName::NoTitle);
        };
        if titles.is_empty() {
            return Err(NotAName::NoTitle);
        }
        if !EXTENSIONS.iter().any(|e| file.ends_with(e)) {
            return Err(NotAName::NotScript((*file).to_owned()));
        }
        let path: RelPath = file.parse().map_err(|e: ValueError| NotAName::NoPath {
            path: (*file).to_owned(),
            why: e.to_string(),
        })?;
        let mut describes = titles
            .iter()
            .map(|t| decode_title(t))
            .collect::<Result<Vec<_>, _>>()?;
        let Some(test) = describes.pop() else {
            return Err(NotAName::NoTitle);
        };
        Ok(Self {
            file: path,
            wanted: Wanted { describes, test },
        })
    }
}

/// The parts of a name between the `::` that no `\` escapes, as written.
fn split_titles(text: &str) -> Vec<&str> {
    let b = text.as_bytes();
    let mut parts = Vec::new();
    let mut from = 0;
    let mut i = 0;
    // Only ASCII bytes are compared, and a part is cut only at a `:`, so every cut
    // is at a character's boundary, even after an escape of a wider character.
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
        } else if b[i] == b':' && b.get(i + 1) == Some(&b':') {
            parts.push(&text[from..i]);
            i += 2;
            from = i;
        } else {
            i += 1;
        }
    }
    parts.push(&text[from.min(text.len())..]);
    parts
}

/// A title as written in a name, its escapes read.
fn decode_title(raw: &str) -> Result<String, NotAName> {
    if raw.is_empty() {
        return Err(NotAName::EmptyTitle);
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(':') => out.push(':'),
            Some('\\') => out.push('\\'),
            Some(other) => return Err(NotAName::BadEscape(format!("\\{other}"))),
            None => return Err(NotAName::BadEscape("\\".to_owned())),
        }
    }
    Ok(out)
}

/// What a name comes to in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Found {
    Text(String),
    Absent,
    CannotTell(Untellable),
}

/// What each of `wanted` comes to in the bytes of the file at `path`: the file is
/// read as text, lexed once and indexed once; then each name is found in the index,
/// and the places of what the reasons name are found in one pass over the text.
fn look_up(path: &RelPath, bytes: Vec<u8>, wanted: &[Wanted]) -> Vec<Found> {
    let every = |why: Untellable| vec![Found::CannotTell(why); wanted.len()];
    let Ok(text) = String::from_utf8(bytes) else {
        return every(Untellable::NotUtf8(path.clone()));
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let jsx = !NO_JSX.iter().any(|e| path.as_str().ends_with(e));
    let lexed = match Lexer::run(text, jsx) {
        Ok(lexed) => lexed,
        Err(LexError::TooDeep) => {
            return every(Untellable::TooDeep {
                path: path.clone(),
                max: MAX_DEPTH,
            });
        }
        Err(LexError::Fault { fault, at }) => {
            let at = places(text, &[at])
                .first()
                .copied()
                .unwrap_or(Place { line: 1, column: 1 });
            return every(Untellable::Lex {
                path: path.clone(),
                fault,
                at,
            });
        }
    };
    let levels = lexed.index();
    let outcomes: Vec<Outcome> = wanted.iter().map(|w| find(&levels, w)).collect();
    let mut offsets: Vec<usize> = outcomes
        .iter()
        .flat_map(|o| match o {
            Outcome::MayBeMade(shown) => shown.first.as_slice(),
            Outcome::Calls(_) | Outcome::Absent => &[],
        })
        .map(|m| lexed.start(wide(m.at)))
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    let at = places(text, &offsets);
    let place_of = |token: u32| {
        offsets
            .binary_search(&lexed.start(wide(token)))
            .ok()
            .and_then(|k| at.get(k).copied())
    };
    outcomes
        .into_iter()
        .map(|outcome| match outcome {
            Outcome::Calls(calls) => Found::Text(
                calls
                    .iter()
                    .map(|(start, close)| lexed.print(wide(*start), wide(*close)))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Outcome::Absent => Found::Absent,
            Outcome::MayBeMade(shown) => Found::CannotTell(Untellable::MayBeMade {
                path: path.clone(),
                by: shown.say(&lexed, &place_of),
            }),
        })
        .collect()
}

/// The places of the byte `offsets` (in order, each at a character) of `src`, in one
/// pass over it.
fn places(src: &str, offsets: &[usize]) -> Vec<Place> {
    let mut out = Vec::with_capacity(offsets.len());
    let mut next = offsets.iter().peekable();
    let (mut line, mut column) = (1, 1);
    let mut after_cr = false;
    for (k, c) in src.char_indices() {
        while next.next_if(|o| **o <= k).is_some() {
            out.push(Place { line, column });
        }
        if next.peek().is_none() {
            return out;
        }
        match c {
            '\n' if after_cr => {}
            c if is_line_end(c) => {
                line += 1;
                column = 1;
            }
            _ => column += 1,
        }
        after_cr = c == '\r';
    }
    out.extend(next.map(|_| Place { line, column }));
    out
}

/// An offset or an index of a lexed file, as a token stores it. Each fits:
/// [`Lexer::run`] refuses a source longer than `u32::MAX` bytes, and a token is at
/// least a byte long, so the fallback is never taken.
fn narrow(x: usize) -> u32 {
    u32::try_from(x).unwrap_or(u32::MAX)
}

/// An offset or an index a token stores, as one of the source. `usize` is at least
/// 32 bits on every target whyshelf builds for, so the fallback is never taken.
fn wide(x: u32) -> usize {
    usize::try_from(x).unwrap_or(usize::MAX)
}

// ---------------------------------------------------------------------------
// The lexer
// ---------------------------------------------------------------------------

/// The kind of a bracket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delim {
    Paren,
    Bracket,
    Brace,
}

impl Delim {
    fn open(self) -> char {
        match self {
            Self::Paren => '(',
            Self::Bracket => '[',
            Self::Brace => '{',
        }
    }

    fn close(self) -> char {
        match self {
            Self::Paren => ')',
            Self::Bracket => ']',
            Self::Brace => '}',
        }
    }
}

/// The kind of a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tk {
    Ident,
    Number,
    Str,
    /// A template literal with no substitution.
    Template,
    /// A template literal up to its first `${`; its partner is the tail.
    TemplateHead,
    /// The `}` … `${` between two substitutions of a template literal.
    TemplateMiddle,
    /// The `}` … `` ` `` that ends a template literal.
    TemplateTail,
    Regex,
    Punct,
    /// The text of an element of JSX, its white space at both ends left out.
    JsxText,
    /// An opening bracket; its partner is the closing one.
    Open(Delim),
    /// A closing bracket; its partner is the opening one.
    Close(Delim),
}

/// One token, in 12 bytes: where it starts; where it ends, but for a bracket, which
/// is one byte long, the index of its partner; its kind; whether a line end comes
/// between it and the token before; and, for `{`, whether it opens a block.
#[derive(Debug, Clone, Copy)]
struct Token {
    start: u32,
    end_or_partner: u32,
    kind: Tk,
    line_before: bool,
    block: bool,
}

/// The tokens of a file, and the partners of the heads of its templates.
#[derive(Debug, Default)]
struct Tokens {
    t: Vec<Token>,
    /// Each head of a template (by its index) and its tail, in the order of the
    /// heads.
    heads: Vec<(u32, u32)>,
}

impl Tokens {
    fn len(&self) -> usize {
        self.t.len()
    }

    fn kind(&self, i: usize) -> Option<Tk> {
        self.t.get(i).map(|k| k.kind)
    }

    fn start(&self, i: usize) -> usize {
        self.t.get(i).map_or(0, |k| wide(k.start))
    }

    fn end(&self, i: usize) -> usize {
        match self.t.get(i) {
            Some(k) if matches!(k.kind, Tk::Open(_) | Tk::Close(_)) => wide(k.start) + 1,
            Some(k) => wide(k.end_or_partner),
            None => 0,
        }
    }

    /// A bracket's other bracket, a template's tail; the token itself for the rest.
    fn partner(&self, i: usize) -> usize {
        match self.t.get(i) {
            Some(k) if matches!(k.kind, Tk::Open(_) | Tk::Close(_)) => wide(k.end_or_partner),
            Some(k) if k.kind == Tk::TemplateHead => {
                let index = narrow(i);
                match self.heads.binary_search_by_key(&index, |(head, _)| *head) {
                    Ok(k) => wide(self.heads[k].1),
                    Err(_) => i,
                }
            }
            _ => i,
        }
    }

    fn line_before(&self, i: usize) -> bool {
        self.t.get(i).is_some_and(|k| k.line_before)
    }
}

/// Why a file does not lex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexError {
    Fault { fault: LexFault, at: usize },
    TooDeep,
}

/// What does not lex, in words of fixed length (a character at most).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexFault {
    String,
    Template,
    Regex,
    Comment,
    Unclosed(Delim),
    Stray(Delim),
    Mismatch(Delim),
    Element,
    StrayTag,
    Char(char),
    TooLong,
}

impl fmt::Display for LexFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String => f.write_str("a string is not closed on its line"),
            Self::Template => f.write_str("a template literal is not closed"),
            Self::Regex => f.write_str("a regular expression is not closed on its line"),
            Self::Comment => f.write_str("a block comment is not closed"),
            Self::Unclosed(d) => write!(f, "a `{}` is not closed", d.open()),
            Self::Stray(d) => write!(f, "a `{}` closes nothing", d.close()),
            Self::Mismatch(d) => write!(f, "a `{}` closes another kind of bracket", d.close()),
            Self::Element => f.write_str("an element of JSX is not closed"),
            Self::StrayTag => f.write_str("a closing tag of JSX closes no element"),
            Self::Char(c) => write!(f, "the character {c:?} starts no token"),
            Self::TooLong => f.write_str("the file is longer than a lexer can index"),
        }
    }
}

fn fault(fault: LexFault, at: usize) -> LexError {
    LexError::Fault { fault, at }
}

/// What is open: a bracket, a substitution of a template, a tag of JSX or the
/// children of an element, each with the token it opened at.
#[derive(Debug, Clone, Copy)]
enum Frame {
    Group(Delim, usize),
    /// A substitution, by the head of its template and the head's place in
    /// [`Tokens::heads`].
    Subst(usize, usize),
    /// A tag (`closing` for `</…>`), by its `<`.
    Tag {
        closing: bool,
        open: usize,
    },
    /// The children of an element, by the `<` of its opening tag.
    Children(usize),
}

/// The punctuators of more than one character, the longest first.
const LONG_PUNCTS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=",
    "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "%=", "&=", "|=", "^=", "<<",
    ">>", "**",
];

/// The punctuators of one character (`/` and the brackets apart).
const PUNCTS: &str = ";,<>+-*%&|^!~?:=.@#";

/// Whether `c` is a line end.
fn is_line_end(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Whether `c` starts an identifier: an ASCII letter, `$`, `_`, the `\` of a `\u`
/// escape, or any character beyond ASCII that is not white space (a lenient reading
/// of `ID_Start`: a character no program may hold there makes no test either).
fn ident_start(c: char) -> bool {
    c.is_ascii_alphabetic()
        || matches!(c, '$' | '_' | '\\')
        || (!c.is_ascii() && !c.is_whitespace() && c != '\u{feff}')
}

fn ident_part(c: char) -> bool {
    ident_start(c) || c.is_ascii_digit()
}

/// The lexer: one loop over the source, what is open on a stack of its own.
struct Lexer<'a> {
    src: &'a str,
    /// Whether the file may hold JSX.
    jsx: bool,
    pos: usize,
    line_before: bool,
    out: Tokens,
    frames: Vec<Frame>,
    /// The depth of frames at which a keyword of [`BODY_KEYWORDS`] waits for its body.
    body_at: Option<usize>,
}

impl<'a> Lexer<'a> {
    /// The tokens of `src`, or why it does not lex.
    fn run(src: &'a str, jsx: bool) -> Result<Lexed<'a>, LexError> {
        if u32::try_from(src.len()).is_err() {
            return Err(fault(LexFault::TooLong, 0));
        }
        let mut lexer = Self {
            src,
            jsx,
            pos: 0,
            line_before: false,
            out: Tokens {
                t: Vec::with_capacity(src.len() / 4),
                heads: Vec::new(),
            },
            frames: Vec::new(),
            body_at: None,
        };
        if src.starts_with("#!") {
            lexer.pos = lexer.line_end(0);
        }
        while lexer.pos < src.len() {
            if matches!(lexer.frames.last(), Some(Frame::Children(_))) {
                lexer.children()?;
                continue;
            }
            let Some(c) = lexer.src[lexer.pos..].chars().next() else {
                break;
            };
            lexer.step(c)?;
        }
        let unclosed = match lexer.frames.last() {
            Some(Frame::Group(d, open)) => Some((LexFault::Unclosed(*d), *open)),
            Some(Frame::Subst(head, _)) => Some((LexFault::Template, *head)),
            Some(Frame::Tag { open, .. } | Frame::Children(open)) => {
                Some((LexFault::Element, *open))
            }
            None => None,
        };
        match unclosed {
            Some((what, open)) => Err(fault(what, lexer.out.start(open))),
            None => Ok(Lexed { src, t: lexer.out }),
        }
    }

    fn in_tag(&self) -> Option<bool> {
        match self.frames.last() {
            Some(Frame::Tag { closing, .. }) => Some(*closing),
            _ => None,
        }
    }

    /// Reads what starts with `c` at the position.
    fn step(&mut self, c: char) -> Result<(), LexError> {
        let start = self.pos;
        let b = self.src.as_bytes();
        let next = b.get(start + 1).copied();
        let tag = self.in_tag();
        match c {
            c if is_line_end(c) => {
                self.line_before = true;
                self.pos += c.len_utf8();
            }
            c if c.is_whitespace() || c == '\u{feff}' => self.pos += c.len_utf8(),
            '/' if next == Some(b'/') && tag.is_none() => self.pos = self.line_end(start),
            '/' if next == Some(b'*') => {
                let Some(len) = self.src[start + 2..].find("*/") else {
                    return Err(fault(LexFault::Comment, start));
                };
                if self.src[start + 2..start + 2 + len].contains(is_line_end) {
                    self.line_before = true;
                }
                self.pos = start + 2 + len + 2;
            }
            '/' if tag.is_some() => self.push(Tk::Punct, start, start + 1),
            '/' => {
                if self.regex_allowed() {
                    let end = self.regex(start)?;
                    self.push(Tk::Regex, start, end);
                } else {
                    let end = if next == Some(b'=') {
                        start + 2
                    } else {
                        start + 1
                    };
                    self.push(Tk::Punct, start, end);
                }
            }
            '>' if tag.is_some() => self.end_tag(start)?,
            ',' if tag == Some(false) => {
                // A list of type parameters, not a tag.
                self.frames.pop();
                self.push(Tk::Punct, start, start + 1);
            }
            '\'' | '"' => {
                let end = self.string(start, b[start], tag.is_some())?;
                self.push(Tk::Str, start, end);
            }
            '`' => self.template(start)?,
            '}' if matches!(self.frames.last(), Some(Frame::Subst(..))) => self.template(start)?,
            '(' => self.open(Delim::Paren, start)?,
            '[' => self.open(Delim::Bracket, start)?,
            '{' => self.open(Delim::Brace, start)?,
            ')' => self.close(Delim::Paren, start)?,
            ']' => self.close(Delim::Bracket, start)?,
            '}' => self.close(Delim::Brace, start)?,
            '0'..='9' => self.number(start),
            '.' if next.is_some_and(|d| d.is_ascii_digit()) => self.number(start),
            '#' if self.src[start + 1..]
                .chars()
                .next()
                .is_some_and(ident_start) =>
            {
                self.ident(start, start + 1)?;
            }
            c if ident_start(c) => self.ident(start, start)?,
            '<' if self.jsx
                && tag.is_none()
                && self.regex_allowed()
                && self.src[start + 1..]
                    .chars()
                    .next()
                    .is_some_and(|d| ident_start(d) || d == '>') =>
            {
                let open = self.out.len();
                self.push(Tk::Punct, start, start + 1);
                self.enter(Frame::Tag {
                    closing: false,
                    open,
                })?;
            }
            _ => self.punct(start, c)?,
        }
        Ok(())
    }

    /// The children of an element: a text up to the next `<` or `{`, then a tag or
    /// code.
    fn children(&mut self) -> Result<(), LexError> {
        let start = self.pos;
        let rest = &self.src[start..];
        let Some(len) = rest.find(['<', '{']) else {
            let open = match self.frames.last() {
                Some(Frame::Children(open)) => self.out.start(*open),
                _ => start,
            };
            return Err(fault(LexFault::Element, open));
        };
        let text = &rest[..len];
        let content = text.trim();
        if !content.is_empty() {
            let lead = text.len() - text.trim_start().len();
            self.push(Tk::JsxText, start + lead, start + lead + content.len());
        }
        if text.contains(is_line_end) {
            self.line_before = true;
        }
        self.pos = start + len;
        let at = self.pos;
        if rest.as_bytes()[len] == b'{' {
            return self.open(Delim::Brace, at);
        }
        let closing = self.src.as_bytes().get(at + 1) == Some(&b'/');
        let open = self.out.len();
        self.push(Tk::Punct, at, at + 1);
        self.enter(Frame::Tag { closing, open })
    }

    /// The `>` at `start` that ends a tag.
    fn end_tag(&mut self, start: usize) -> Result<(), LexError> {
        let self_closing = self.out.t.last().is_some_and(|k| {
            k.kind == Tk::Punct && &self.src[wide(k.start)..self.out.end(self.out.len() - 1)] == "/"
        });
        let Some(Frame::Tag { closing, open }) = self.frames.pop() else {
            return Err(fault(LexFault::StrayTag, start));
        };
        self.push(Tk::Punct, start, start + 1);
        if closing {
            match self.frames.pop() {
                Some(Frame::Children(_)) => Ok(()),
                _ => Err(fault(LexFault::StrayTag, self.out.start(open))),
            }
        } else if self_closing {
            Ok(())
        } else {
            self.enter(Frame::Children(open))
        }
    }

    /// Adds a token from `start` to `end`, and moves past it.
    fn push(&mut self, kind: Tk, start: usize, end: usize) {
        let index = self.out.len();
        self.out.t.push(Token {
            start: narrow(start),
            end_or_partner: if matches!(kind, Tk::Open(_) | Tk::Close(_)) {
                narrow(index)
            } else {
                narrow(end)
            },
            kind,
            line_before: std::mem::take(&mut self.line_before),
            block: false,
        });
        self.pos = end;
    }

    fn enter(&mut self, frame: Frame) -> Result<(), LexError> {
        self.frames.push(frame);
        if self.frames.len() > MAX_DEPTH {
            return Err(LexError::TooDeep);
        }
        Ok(())
    }

    /// The text of the token at `i`.
    fn text(&self, i: usize) -> &str {
        self.src
            .get(self.out.start(i)..self.out.end(i))
            .unwrap_or("")
    }

    fn is(&self, i: usize, kind: Tk, text: &str) -> bool {
        self.out.kind(i) == Some(kind) && self.text(i) == text
    }

    /// Whether the token at `i` is a name after `.` or `?.` (a property, whatever
    /// its word).
    fn after_dot(&self, i: usize) -> bool {
        i > 0 && (self.is(i - 1, Tk::Punct, ".") || self.is(i - 1, Tk::Punct, "?."))
    }

    /// Whether the `(` at `open` starts the head of `for` (or `for await`).
    fn after_for(&self, open: usize) -> bool {
        let Some(k) = open.checked_sub(1) else {
            return false;
        };
        (self.is(k, Tk::Ident, "for") && !self.after_dot(k))
            || (self.is(k, Tk::Ident, "await") && k > 0 && self.is(k - 1, Tk::Ident, "for"))
    }

    /// Whether the `(` at `open` starts the head of `if`, `while`, `for`, `with` (or
    /// `for await`), after which an operand starts a statement.
    fn after_control(&self, open: usize) -> bool {
        let Some(k) = open.checked_sub(1) else {
            return false;
        };
        self.after_for(open)
            || (self.out.kind(k) == Some(Tk::Ident)
                && !self.after_dot(k)
                && matches!(self.text(k), "if" | "while" | "with"))
    }

    /// Whether the token at `i` (an identifier) is a keyword after which an operand
    /// starts: `of` only in the head of a `for`.
    fn operand_keyword(&self, i: usize) -> bool {
        let word = self.text(i);
        if self.after_dot(i) || !OPERAND_KEYWORDS.contains(&word) {
            return false;
        }
        if word != "of" {
            return true;
        }
        matches!(self.frames.last(), Some(Frame::Group(Delim::Paren, open)) if self.after_for(*open))
    }

    /// Whether the token at `i` ends an operand.
    fn ends_operand(&self, i: usize) -> bool {
        match self.out.kind(i) {
            Some(
                Tk::Number
                | Tk::Str
                | Tk::Template
                | Tk::TemplateTail
                | Tk::Regex
                | Tk::Close(Delim::Paren | Delim::Bracket),
            ) => true,
            Some(Tk::Ident) => !self.operand_keyword(i),
            _ => false,
        }
    }

    /// Whether a `/` here starts a regular expression, and a `<` here may start a
    /// tag (the table on the page of the module).
    fn regex_allowed(&self) -> bool {
        let Some(last) = self.out.len().checked_sub(1) else {
            return true;
        };
        let prev = self.out.t[last];
        match prev.kind {
            Tk::Number | Tk::Str | Tk::Template | Tk::TemplateTail | Tk::Regex | Tk::JsxText => {
                false
            }
            Tk::TemplateHead | Tk::TemplateMiddle | Tk::Open(_) => true,
            Tk::Close(Delim::Paren) => self.after_control(self.out.partner(last)),
            Tk::Close(Delim::Bracket) => false,
            Tk::Close(Delim::Brace) => self.out.t[self.out.partner(last)].block,
            Tk::Punct => match self.text(last) {
                "++" | "--" => false,
                // TypeScript's non-null: `total!` then division.
                "!" => {
                    !(last > 0
                        && self.ends_operand(last - 1)
                        && self.out.end(last - 1) == self.out.start(last))
                }
                _ => true,
            },
            Tk::Ident => {
                self.operand_keyword(last)
                    || (!self.after_dot(last) && matches!(self.text(last), "do" | "else"))
            }
        }
    }

    /// Whether a `{` here opens a block rather than an object literal (the rule on
    /// the page of the module).
    fn brace_is_block(&self) -> bool {
        if self.body_at == Some(self.frames.len()) {
            return true;
        }
        let Some(last) = self.out.len().checked_sub(1) else {
            return true;
        };
        match self.out.t[last].kind {
            Tk::Open(Delim::Brace) | Tk::Close(Delim::Brace) | Tk::Close(Delim::Paren) => true,
            Tk::Open(_) | Tk::Close(Delim::Bracket) | Tk::TemplateHead | Tk::TemplateMiddle => {
                false
            }
            Tk::Punct => matches!(self.text(last), ";" | "=>"),
            Tk::Ident => !self.operand_keyword(last),
            Tk::Number | Tk::Str | Tk::Template | Tk::TemplateTail | Tk::Regex | Tk::JsxText => {
                true
            }
        }
    }

    /// Where the line that `from` is on ends (at its line end, or the end of the
    /// source).
    fn line_end(&self, from: usize) -> usize {
        self.src[from..]
            .find(is_line_end)
            .map_or(self.src.len(), |k| from + k)
    }

    fn open(&mut self, d: Delim, start: usize) -> Result<(), LexError> {
        let index = self.out.len();
        let block = d == Delim::Brace && self.brace_is_block();
        if d == Delim::Brace && self.body_at == Some(self.frames.len()) {
            self.body_at = None;
        }
        self.push(Tk::Open(d), start, start + 1);
        self.out.t[index].block = block;
        self.enter(Frame::Group(d, index))
    }

    fn close(&mut self, d: Delim, start: usize) -> Result<(), LexError> {
        match self.frames.pop() {
            Some(Frame::Group(open_d, open)) if open_d == d => {
                let index = self.out.len();
                self.push(Tk::Close(d), start, start + 1);
                self.out.t[open].end_or_partner = narrow(index);
                self.out.t[index].end_or_partner = narrow(open);
                Ok(())
            }
            Some(_) => Err(fault(LexFault::Mismatch(d), start)),
            None => Err(fault(LexFault::Stray(d), start)),
        }
    }

    /// The end of the string that starts with `quote` at `start`; in a tag of JSX
    /// it may span lines and holds no escapes. The bytes looked at are ASCII, so an
    /// escape of a wider character steps into it harmlessly: its other bytes are
    /// none of them.
    fn string(&self, start: usize, quote: u8, in_tag: bool) -> Result<usize, LexError> {
        let b = self.src.as_bytes();
        let mut p = start + 1;
        loop {
            match b.get(p) {
                None => return Err(fault(LexFault::String, start)),
                Some(&q) if q == quote => return Ok(p + 1),
                Some(_) if in_tag => p += 1,
                Some(b'\n' | b'\r') => return Err(fault(LexFault::String, start)),
                Some(b'\\') => {
                    p += if b.get(p + 1) == Some(&b'\r') && b.get(p + 2) == Some(&b'\n') {
                        3
                    } else {
                        2
                    };
                }
                Some(_) => p += 1,
            }
        }
    }

    /// The end of the regular expression literal that starts at `start`, its flags
    /// included.
    fn regex(&self, start: usize) -> Result<usize, LexError> {
        let b = self.src.as_bytes();
        let mut p = start + 1;
        let mut class = false;
        loop {
            match b.get(p) {
                None | Some(b'\n' | b'\r') => return Err(fault(LexFault::Regex, start)),
                Some(b'\\') => {
                    if matches!(b.get(p + 1), None | Some(b'\n' | b'\r')) {
                        return Err(fault(LexFault::Regex, start));
                    }
                    p += 2;
                }
                Some(b'[') => {
                    class = true;
                    p += 1;
                }
                Some(b']') => {
                    class = false;
                    p += 1;
                }
                Some(b'/') if !class => {
                    p += 1;
                    break;
                }
                Some(_) => p += 1,
            }
        }
        while b
            .get(p)
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, b'_' | b'$'))
        {
            p += 1;
        }
        Ok(p)
    }

    /// A part of a template literal that starts at `start`: its `` ` ``, or the `}`
    /// that closes a substitution.
    fn template(&mut self, start: usize) -> Result<(), LexError> {
        let b = self.src.as_bytes();
        let continuing = b[start] == b'}';
        let opened_at = match self.frames.last() {
            Some(Frame::Subst(head, _)) if continuing => self.out.start(*head),
            _ => start,
        };
        let mut p = start + 1;
        let (end, closed) = loop {
            match b.get(p) {
                None => return Err(fault(LexFault::Template, opened_at)),
                Some(b'`') => break (p + 1, true),
                Some(b'\\') => p += 2,
                Some(b'$') if b.get(p + 1) == Some(&b'{') => break (p + 2, false),
                Some(_) => p += 1,
            }
        };
        let index = self.out.len();
        match (continuing, closed) {
            (false, true) => self.push(Tk::Template, start, end),
            (false, false) => {
                self.push(Tk::TemplateHead, start, end);
                let slot = self.out.heads.len();
                self.out.heads.push((narrow(index), narrow(index)));
                self.enter(Frame::Subst(index, slot))?;
            }
            (true, false) => self.push(Tk::TemplateMiddle, start, end),
            (true, true) => {
                let Some(Frame::Subst(_, slot)) = self.frames.pop() else {
                    return Err(fault(LexFault::Template, start));
                };
                self.push(Tk::TemplateTail, start, end);
                if let Some(head) = self.out.heads.get_mut(slot) {
                    head.1 = narrow(index);
                }
            }
        }
        Ok(())
    }

    fn number(&mut self, start: usize) {
        let b = self.src.as_bytes();
        let radix = b[start] == b'0'
            && matches!(
                b.get(start + 1),
                Some(b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
            );
        let mut dot = false;
        let mut p = start;
        loop {
            match b.get(p) {
                Some(c) if c.is_ascii_alphanumeric() || *c == b'_' => p += 1,
                Some(b'.') if !dot && !radix => {
                    dot = true;
                    p += 1;
                }
                Some(b'+' | b'-') if !radix && p > start && matches!(b[p - 1], b'e' | b'E') => {
                    p += 1;
                }
                _ => break,
            }
        }
        self.push(Tk::Number, start, p);
    }

    /// An identifier whose characters start at `from` (after the `#` of a private
    /// name, which `start` is at).
    fn ident(&mut self, start: usize, from: usize) -> Result<(), LexError> {
        let b = self.src.as_bytes();
        let mut p = from;
        while let Some(c) = self.src[p..].chars().next() {
            if !ident_part(c) {
                break;
            }
            if c == '\\' {
                if b.get(p + 1) != Some(&b'u') {
                    return Err(fault(LexFault::Char('\\'), p));
                }
                p += 2;
                if b.get(p) == Some(&b'{') {
                    match self.src[p..].find('}') {
                        Some(k) => p += k + 1,
                        None => return Err(fault(LexFault::Char('\\'), p - 2)),
                    }
                }
                continue;
            }
            p += c.len_utf8();
        }
        let index = self.out.len();
        self.push(Tk::Ident, start, p);
        let word = &self.src[start..p];
        if !self.after_dot(index) {
            if BODY_KEYWORDS.contains(&word) {
                self.body_at = Some(self.frames.len());
            } else if word == "extends" && self.in_tag() == Some(false) {
                // A list of type parameters (`<T extends U>`), not a tag.
                self.frames.pop();
            }
        }
        Ok(())
    }

    fn punct(&mut self, start: usize, c: char) -> Result<(), LexError> {
        let rest = &self.src[start..];
        let long = LONG_PUNCTS.iter().find(|p| {
            rest.starts_with(**p)
                && !(**p == "?." && rest.as_bytes().get(2).is_some_and(|d| d.is_ascii_digit()))
        });
        if let Some(p) = long {
            self.push(Tk::Punct, start, start + p.len());
            return Ok(());
        }
        if PUNCTS.contains(c) {
            if c == ';' && self.body_at == Some(self.frames.len()) {
                self.body_at = None;
            }
            self.push(Tk::Punct, start, start + 1);
            return Ok(());
        }
        Err(fault(LexFault::Char(c), start))
    }
}

/// The value of a string literal (`template` false) or of a template literal with
/// no substitution, written `raw` between its quotes or backticks, as JavaScript
/// reads it; none when it holds an escape a template may not hold, or its value is
/// no Unicode text (a lone surrogate): such a title is not literal.
fn decode(raw: &str, template: bool) -> Option<String> {
    fn push(units: &mut Vec<u16>, c: char) {
        let mut buf = [0_u16; 2];
        units.extend_from_slice(c.encode_utf16(&mut buf));
    }
    fn hex(c: Option<char>) -> Option<u32> {
        c?.to_digit(16)
    }
    let mut units: Vec<u16> = Vec::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' && template {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            units.push(0x0A);
            continue;
        }
        if c != '\\' {
            push(&mut units, c);
            continue;
        }
        let e = chars.next()?;
        match e {
            'n' => units.push(0x0A),
            't' => units.push(0x09),
            'r' => units.push(0x0D),
            'b' => units.push(0x08),
            'f' => units.push(0x0C),
            'v' => units.push(0x0B),
            '0' if !chars.peek().is_some_and(char::is_ascii_digit) => units.push(0),
            '0'..='7' if !template => {
                // A legacy octal escape: up to three digits from 0–3, two from 4–7.
                let mut value = e.to_digit(8)?;
                let most = if value < 4 { 2 } else { 1 };
                for _ in 0..most {
                    match chars.peek().and_then(|d| d.to_digit(8)) {
                        Some(d) => {
                            value = value * 8 + d;
                            chars.next();
                        }
                        None => break,
                    }
                }
                units.push(u16::try_from(value).ok()?);
            }
            '8' | '9' if !template => push(&mut units, e),
            '0'..='9' => return None,
            'x' => {
                let value = hex(chars.next())? * 16 + hex(chars.next())?;
                units.push(u16::try_from(value).ok()?);
            }
            'u' if chars.peek() == Some(&'{') => {
                chars.next();
                let mut value: u32 = 0;
                let mut digits = 0;
                loop {
                    match chars.next()? {
                        '}' if digits > 0 => break,
                        d => {
                            value = value.checked_mul(16)?.checked_add(d.to_digit(16)?)?;
                            digits += 1;
                            if value > 0x10_FFFF {
                                return None;
                            }
                        }
                    }
                }
                match char::from_u32(value) {
                    Some(c) => push(&mut units, c),
                    // A surrogate, written alone: kept as the unit it is.
                    None => units.push(u16::try_from(value).ok()?),
                }
            }
            'u' => {
                let mut value = 0;
                for _ in 0..4 {
                    value = value * 16 + hex(chars.next())?;
                }
                units.push(u16::try_from(value).ok()?);
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => {}
            other => push(&mut units, other),
        }
    }
    String::from_utf16(&units).ok()
}

// ---------------------------------------------------------------------------
// The index
// ---------------------------------------------------------------------------

/// The role of a test call: a `describe` holds tests, a test is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Describe,
    Test,
}

impl Role {
    /// Its place in [`Level::roles`].
    fn slot(self) -> usize {
        match self {
            Self::Describe => 0,
            Self::Test => 1,
        }
    }
}

/// Where the children of a `describe` are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Body {
    /// It has no argument after its title.
    None,
    /// The `{` of the body of its callback.
    Brace(usize),
    /// It has arguments after its title, and none is a function with a brace body.
    Unreadable,
}

/// What a test call on a level comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    /// A `describe` or a test, its title when it is literal, and where its children
    /// are.
    Made {
        role: Role,
        close: usize,
        title: Option<String>,
        body: Body,
    },
    /// A `.each` of the role.
    Each(Role),
    /// An annotation of Playwright, which makes nothing.
    Harmless,
    /// A call of the role whose list of type arguments is too long to step over.
    TooLong(Role),
}

/// What a list of type arguments comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Angles {
    /// It ends, before the token at this index.
    Ends(usize),
    /// It is no list: a `;` or the end of the file comes first.
    NoList,
    /// It runs past [`MAX_TYPE_ARGUMENTS`] tokens.
    TooLong,
}

/// What may make a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum What {
    /// A call of the role whose title is not literal.
    NotLiteral,
    /// A `.each` of the role.
    Each,
    /// A call of another function that starts a statement.
    Helper,
    /// A `describe` of the title sought whose callback cannot be read.
    Unreadable,
    /// A call of the role whose type arguments are too long to be stepped over.
    TooLong,
}

/// One thing that may make a test, by the first token of its callee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Maker {
    what: What,
    at: u32,
}

/// The makers of one kind on a level: the first [`SHOWN`] of them, and how many
/// there are in all.
#[derive(Debug, Default)]
struct Seen {
    first: Vec<Maker>,
    count: usize,
}

impl Seen {
    fn add(&mut self, maker: Maker) {
        if self.first.len() < SHOWN {
            self.first.push(maker);
        }
        self.count = self.count.saturating_add(1);
    }
}

/// A `describe` of a title on a level: its own level, or one whose callback cannot
/// be read (by the first token of its callee).
#[derive(Debug, Clone, Copy)]
enum Child {
    Level(usize),
    Unreadable(u32),
}

/// One level, indexed: its `describe`s and tests by title, and what on it may make
/// a `describe` (by [`Role::slot`]) or a test, and its calls of helpers.
#[derive(Debug, Default)]
struct Level {
    describes: HashMap<String, Vec<Child>>,
    /// Each test of a title: the tokens of its callee and of its closing `)`.
    tests: HashMap<String, Vec<(u32, u32)>>,
    roles: [Seen; 2],
    helpers: Seen,
}

/// What may make a test on the way of a name: the first [`SHOWN`], and how many
/// more. Every maker is one call or title of the file, counted once.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Shown {
    first: Vec<Maker>,
    more: usize,
}

impl Shown {
    /// Takes the makers of a role on a level and its helpers, in the order of the
    /// file.
    fn take(&mut self, a: &Seen, b: &Seen) {
        let (mut x, mut y) = (a.first.iter().peekable(), b.first.iter().peekable());
        let mut taken = 0;
        while self.first.len() < SHOWN {
            let next = match (x.peek(), y.peek()) {
                (Some(m), Some(n)) if m.at <= n.at => x.next(),
                (Some(_), Some(_)) | (None, Some(_)) => y.next(),
                (Some(_), None) => x.next(),
                (None, None) => None,
            };
            let Some(maker) = next else {
                break;
            };
            self.first.push(*maker);
            taken += 1;
        }
        self.more = self
            .more
            .saturating_add((a.count + b.count).saturating_sub(taken));
    }

    fn one(&mut self, maker: Maker) {
        if self.first.len() < SHOWN {
            self.first.push(maker);
        } else {
            self.more = self.more.saturating_add(1);
        }
    }

    fn is_empty(&self) -> bool {
        self.first.is_empty()
    }

    /// The makers as a reason says them, with the place of each.
    fn say(&self, lexed: &Lexed<'_>, place_of: &dyn Fn(u32) -> Option<Place>) -> String {
        let mut parts: Vec<String> = self
            .first
            .iter()
            .map(|m| {
                let at = match place_of(m.at) {
                    Some(p) => format!("line {}, column {}", p.line, p.column),
                    None => "a place not found".to_owned(),
                };
                let i = wide(m.at);
                match m.what {
                    What::NotLiteral => format!(
                        "`{}` ({at}), whose title is not a literal",
                        lexed.callee_words(i)
                    ),
                    What::Each => format!("`{}` ({at})", lexed.callee_words(i)),
                    What::Helper => {
                        format!("the call of `{}` ({at})", lexed.chain(i).callee)
                    }
                    What::Unreadable => format!(
                        "`{}` ({at}), whose callback is not a function with a body",
                        lexed.callee_words(i)
                    ),
                    What::TooLong => format!(
                        "`{}` ({at}), whose type arguments are longer than {MAX_TYPE_ARGUMENTS} tokens, so its call is not read",
                        lexed.callee_words(i)
                    ),
                }
            })
            .collect();
        if self.more > 0 {
            parts.push(format!("and {} more", self.more));
        }
        parts.join("; ")
    }
}

/// What a name comes to in the index of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// The test calls of the name, by the tokens of their callee and closing `)`.
    Calls(Vec<(u32, u32)>),
    Absent,
    MayBeMade(Shown),
}

/// The test `wanted` in the index `levels` (the rules on the page of the module).
fn find(levels: &[Level], wanted: &Wanted) -> Outcome {
    let mut shown = Shown::default();
    let mut current: Vec<usize> = vec![0];
    let not_found = |shown: Shown| {
        if shown.is_empty() {
            Outcome::Absent
        } else {
            Outcome::MayBeMade(shown)
        }
    };
    for title in &wanted.describes {
        let mut next = Vec::new();
        for level in current.iter().filter_map(|id| levels.get(*id)) {
            shown.take(&level.roles[Role::Describe.slot()], &level.helpers);
            for child in level.describes.get(title).into_iter().flatten() {
                match child {
                    Child::Level(id) => next.push(*id),
                    Child::Unreadable(at) => shown.one(Maker {
                        what: What::Unreadable,
                        at: *at,
                    }),
                }
            }
        }
        if next.is_empty() {
            return not_found(shown);
        }
        current = next;
    }
    let mut calls = Vec::new();
    for level in current.iter().filter_map(|id| levels.get(*id)) {
        shown.take(&level.roles[Role::Test.slot()], &level.helpers);
        calls.extend(level.tests.get(&wanted.test).into_iter().flatten().copied());
    }
    if calls.is_empty() {
        not_found(shown)
    } else {
        Outcome::Calls(calls)
    }
}

/// The text between the quotes, or backticks, of a literal.
fn strip_ends(literal: &str) -> &str {
    literal
        .get(1..literal.len().saturating_sub(1))
        .unwrap_or("")
}

/// `text`, cut to at most `most` characters, with `…` when it is cut.
fn bounded(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

/// A chain of a call of a helper: where it ends, whether it calls and makes no test,
/// and its callee as a reason writes it.
struct Chain {
    end: usize,
    helper: bool,
    callee: String,
}

/// The levels left to index, by their id and their tokens, and the next id.
struct Queue {
    next_id: usize,
    work: Vec<(usize, usize, usize)>,
}

/// A file lexed: its source and its tokens.
struct Lexed<'a> {
    src: &'a str,
    t: Tokens,
}

impl Lexed<'_> {
    fn text(&self, i: usize) -> &str {
        self.src.get(self.t.start(i)..self.t.end(i)).unwrap_or("")
    }

    fn start(&self, i: usize) -> usize {
        self.t.start(i)
    }

    fn kind(&self, i: usize) -> Option<Tk> {
        self.t.kind(i)
    }

    fn is_punct(&self, i: usize, p: &str) -> bool {
        self.kind(i) == Some(Tk::Punct) && self.text(i) == p
    }

    fn is_word(&self, i: usize, w: &str) -> bool {
        self.kind(i) == Some(Tk::Ident) && self.text(i) == w && !self.after_dot(i)
    }

    /// The partner of the bracket `d` that opens at `i`.
    fn group(&self, i: usize, d: Delim) -> Option<usize> {
        (self.kind(i) == Some(Tk::Open(d))).then(|| self.t.partner(i))
    }

    fn after_dot(&self, i: usize) -> bool {
        i > 0 && self.kind(i - 1) == Some(Tk::Punct) && matches!(self.text(i - 1), "." | "?.")
    }

    /// The token after the one at `i`, a group or a template being one.
    fn skip(&self, i: usize) -> usize {
        match self.kind(i) {
            Some(Tk::Open(_) | Tk::TemplateHead) => self.t.partner(i) + 1,
            _ => i + 1,
        }
    }

    /// The text of the tokens from `start` to `close`, one space between two, or a
    /// line end where one changes what the code does (the rule on the page of the
    /// module).
    fn print(&self, start: usize, close: usize) -> String {
        let mut out = String::new();
        for k in start..=close.min(self.t.len().saturating_sub(1)) {
            if k > start {
                let kept = self.t.line_before(k)
                    && ((self.kind(k - 1) == Some(Tk::Ident)
                        && !self.after_dot(k - 1)
                        && RESTRICTED.contains(&self.text(k - 1)))
                        || self.is_punct(k, "++")
                        || self.is_punct(k, "--"));
                out.push(if kept { '\n' } else { ' ' });
            }
            out.push_str(self.text(k));
        }
        out
    }

    /// The names of a callee from `i`: its first and the members after it
    /// (`test.describe.serial`), at most 60 characters.
    fn callee_words(&self, i: usize) -> String {
        let mut words = self.text(i).to_owned();
        let mut j = i + 1;
        while self.is_punct(j, ".") && self.kind(j + 1) == Some(Tk::Ident) && words.len() < 64 {
            words.push('.');
            words.push_str(self.text(j + 1));
            j += 2;
        }
        bounded(&words, 60)
    }

    /// Whether the `(` at `open` starts the head of a statement among `words` (or
    /// `for await`).
    fn control_head(&self, open: usize, words: &[&str]) -> bool {
        let Some(k) = open.checked_sub(1) else {
            return false;
        };
        (self.kind(k) == Some(Tk::Ident) && !self.after_dot(k) && words.contains(&self.text(k)))
            || (self.is_word(k, "await") && k > 0 && self.is_word(k - 1, "for"))
    }

    /// Whether the token at `i` starts a statement of the level that starts at `lo`
    /// (the rule on the page of the module), `await` and `void` before it skipped;
    /// `colon` is the `:` of the case or the label last seen.
    fn starts_statement(&self, i: usize, lo: usize, colon: usize) -> bool {
        let mut k = i;
        loop {
            if k == lo {
                return true;
            }
            let p = k - 1;
            match self.kind(p) {
                Some(Tk::Punct) if self.text(p) == ";" || p == colon => return true,
                Some(Tk::Open(Delim::Brace) | Tk::Close(Delim::Brace)) => return true,
                Some(Tk::Close(Delim::Paren))
                    if self.control_head(self.t.partner(p), &["if", "for", "while", "with"]) =>
                {
                    return true;
                }
                Some(Tk::Ident) if !self.after_dot(p) => match self.text(p) {
                    "else" | "do" => return true,
                    "await" | "void" => {
                        k = p;
                        continue;
                    }
                    _ => {}
                },
                _ => {}
            }
            let ends_operand = match self.kind(p) {
                Some(
                    Tk::Ident
                    | Tk::Number
                    | Tk::Str
                    | Tk::Template
                    | Tk::TemplateTail
                    | Tk::Regex
                    | Tk::JsxText
                    | Tk::Close(_),
                ) => true,
                Some(Tk::Punct) => matches!(self.text(p), "++" | "--"),
                _ => false,
            };
            return self.t.line_before(k) && self.kind(k) == Some(Tk::Ident) && ends_operand;
        }
    }

    /// Whether the `{` at `i` opens a block of a statement of the level that starts
    /// at `lo`, read as part of the level (the rule on the page of the module).
    fn statement_block(&self, i: usize, lo: usize, colon: usize) -> bool {
        if i == lo {
            return true;
        }
        let p = i - 1;
        match self.kind(p) {
            Some(Tk::Punct) => self.text(p) == ";" || p == colon,
            Some(Tk::Open(Delim::Brace) | Tk::Close(Delim::Brace)) => true,
            Some(Tk::Close(Delim::Paren)) => self.control_head(
                self.t.partner(p),
                &["if", "for", "while", "with", "switch", "catch"],
            ),
            Some(Tk::Ident) => {
                (!self.after_dot(p)
                    && matches!(self.text(p), "else" | "do" | "try" | "finally" | "catch"))
                    || self.namespace_head(p)
            }
            Some(Tk::Str) => {
                p > 1 && self.is_word(p - 1, "module") && self.is_word(p - 2, "declare")
            }
            _ => false,
        }
    }

    /// Whether the name at `p` ends the head of a namespace (`namespace A.B`,
    /// `module A`, `declare global`).
    fn namespace_head(&self, p: usize) -> bool {
        if self.text(p) == "global" && p > 0 && self.is_word(p - 1, "declare") {
            return true;
        }
        let mut k = p;
        while k >= 2 && self.is_punct(k - 1, ".") && self.kind(k - 2) == Some(Tk::Ident) {
            k -= 2;
        }
        k > 0 && (self.is_word(k - 1, "namespace") || self.is_word(k - 1, "module"))
    }

    /// The list of type arguments that starts with the `<` at `j`, read for at most
    /// [`MAX_TYPE_ARGUMENTS`] tokens.
    fn skip_angles(&self, j: usize) -> Angles {
        let mut depth: isize = 0;
        let mut k = j;
        for _ in 0..MAX_TYPE_ARGUMENTS {
            if k >= self.t.len() {
                return Angles::NoList;
            }
            if self.kind(k) == Some(Tk::Punct) {
                match self.text(k) {
                    "<" => depth += 1,
                    ">" => depth -= 1,
                    ">>" => depth -= 2,
                    ">>>" => depth -= 3,
                    ";" => return Angles::NoList,
                    _ => {}
                }
                if depth <= 0 {
                    return Angles::Ends(k + 1);
                }
            }
            k = self.skip(k);
        }
        Angles::TooLong
    }

    /// The `{` of the body of a function whose parameters end before `from`, before
    /// `limit`: the first brace at the top that does not follow a punctuator (but
    /// `>`), so that a return type's braces (`): { done: boolean } {`) are stepped
    /// over; none when a `;` or another `function` comes first.
    fn body_after(&self, from: usize, limit: usize) -> Option<usize> {
        let mut k = from;
        while k < limit {
            match self.kind(k) {
                Some(Tk::Punct) if self.text(k) == ";" => return None,
                Some(Tk::Ident) if self.text(k) == "function" => return None,
                Some(Tk::Open(Delim::Brace)) => {
                    let typed = self.kind(k - 1) == Some(Tk::Punct) && self.text(k - 1) != ">";
                    if !typed {
                        return Some(k);
                    }
                }
                _ => {}
            }
            k = self.skip(k);
        }
        None
    }

    /// The parameters of a function whose `function` is at `i`: its `(`, after its
    /// `*`, name and type parameters.
    fn function_params(&self, i: usize) -> Option<usize> {
        let mut j = i + 1;
        if self.is_punct(j, "*") {
            j += 1;
        }
        if self.kind(j) == Some(Tk::Ident) {
            j += 1;
        }
        if self.is_punct(j, "<") {
            let Angles::Ends(after) = self.skip_angles(j) else {
                return None;
            };
            j = after;
        }
        (self.kind(j) == Some(Tk::Open(Delim::Paren))).then_some(j)
    }

    /// The token after a function whose `function` is at `i`, its body with it.
    fn skip_function(&self, i: usize, hi: usize) -> usize {
        let Some(open) = self.function_params(i) else {
            return i + 1;
        };
        let params = self.t.partner(open);
        match self.body_after(params + 1, hi) {
            Some(body) => self.t.partner(body) + 1,
            None => params + 1,
        }
    }

    /// The `:` that ends the head of a `case` from `from`, before `hi`; none
    /// (`usize::MAX`) when a `;`, another `case` or the end comes first.
    fn case_colon(&self, from: usize, hi: usize) -> usize {
        let mut questions = 0_usize;
        let mut k = from;
        while k < hi {
            match self.kind(k) {
                Some(Tk::Punct) => match self.text(k) {
                    "?" => questions += 1,
                    ":" if questions == 0 => return k,
                    ":" => questions -= 1,
                    ";" => return usize::MAX,
                    _ => {}
                },
                Some(Tk::Ident) if matches!(self.text(k), "case" | "default") => {
                    return usize::MAX;
                }
                _ => {}
            }
            k = self.skip(k);
        }
        usize::MAX
    }

    /// The chain of a call of a helper that starts at `i` (the rules on the page of
    /// the module).
    fn chain(&self, i: usize) -> Chain {
        let (mut callee, mut j) = match self.kind(i) {
            Some(Tk::Ident) => (self.text(i).to_owned(), i + 1),
            Some(Tk::Open(Delim::Paren)) => ("(…)".to_owned(), self.t.partner(i) + 1),
            Some(Tk::Open(Delim::Bracket)) => ("[…]".to_owned(), self.t.partner(i) + 1),
            _ => {
                return Chain {
                    end: i + 1,
                    helper: false,
                    callee: String::new(),
                };
            }
        };
        let mut members: Vec<usize> = Vec::new();
        let mut called = false;
        loop {
            if (self.is_punct(j, ".") || self.is_punct(j, "?."))
                && self.kind(j + 1) == Some(Tk::Ident)
            {
                if !called {
                    if members.len() < 2 {
                        members.push(j + 1);
                    }
                    if callee.len() < 64 {
                        callee.push('.');
                        callee.push_str(self.text(j + 1));
                    }
                }
                j += 2;
            } else if self.is_punct(j, "!")
                && (self.is_punct(j + 1, ".") || self.kind(j + 1) == Some(Tk::Open(Delim::Paren)))
            {
                j += 1;
            } else if let Some(close) = self.group(j, Delim::Paren) {
                called = true;
                j = close + 1;
            } else if let Some(close) = self.group(j, Delim::Bracket) {
                if !called && callee.len() < 64 {
                    callee.push_str("[…]");
                }
                j = close + 1;
            } else if matches!(self.kind(j), Some(Tk::Template | Tk::TemplateHead)) {
                called = true;
                j = self.skip(j);
            } else {
                break;
            }
        }
        let root = if self.kind(i) == Some(Tk::Ident) {
            self.text(i)
        } else {
            ""
        };
        let member = |k: usize| members.get(k).map_or("", |m| self.text(*m));
        let harmless = HARMLESS.contains(&root)
            || (root == "test"
                && (PLAYWRIGHT_HARMLESS.contains(&member(0))
                    || (member(0) == "describe" && member(1) == "configure")));
        Chain {
            end: j,
            helper: called && !harmless,
            callee: bounded(&callee, 60),
        }
    }

    /// The test call whose callee starts at `i` (the rules on the page of the
    /// module), and the token after it; none when there is none there.
    fn test_call(&self, i: usize) -> Option<(Call, usize)> {
        let word = self.text(i);
        let mut role = if DESCRIBES.contains(&word) {
            Role::Describe
        } else if TESTS.contains(&word) {
            Role::Test
        } else {
            return None;
        };
        if self.after_dot(i)
            || (i > 0 && (self.is_word(i - 1, "function") || self.is_word(i - 1, "class")))
        {
            return None;
        }
        let mut j = i + 1;
        let mut playwright = false;
        let mut annotation = false;
        let mut first = true;
        while self.is_punct(j, ".") && self.kind(j + 1) == Some(Tk::Ident) {
            let member = self.text(j + 1);
            if first && word == "test" && member == "describe" {
                role = Role::Describe;
                playwright = true;
                first = false;
                j += 2;
                continue;
            }
            first = false;
            let modifier = if playwright {
                PLAYWRIGHT_DESCRIBE_MODIFIERS.contains(&member)
            } else {
                MODIFIERS.contains(&member)
                    || (role == Role::Test && TEST_MODIFIERS.contains(&member))
            };
            if modifier {
                // Only Playwright's `test` has annotations.
                annotation |= word == "test" && role == Role::Test && ANNOTATIONS.contains(&member);
                j += 2;
            } else if !playwright && matches!(member, "skipIf" | "runIf") {
                j = self.group(j + 2, Delim::Paren)? + 1;
            } else if !playwright && member == "each" {
                j += 2;
                if let Some(close) = self.group(j, Delim::Paren) {
                    j = close + 1;
                } else if matches!(self.kind(j), Some(Tk::Template | Tk::TemplateHead)) {
                    j = self.skip(j);
                }
                if let Some(close) = self.group(j, Delim::Paren) {
                    j = close + 1;
                }
                return Some((Call::Each(role), j));
            } else {
                return None;
            }
        }
        if self.is_punct(j, "<") {
            match self.skip_angles(j) {
                Angles::Ends(after) => j = after,
                Angles::NoList => return None,
                // A test may be there: its call is not read, and is told so.
                Angles::TooLong => return Some((Call::TooLong(role), j + 1)),
            }
        }
        let close = self.group(j, Delim::Paren)?;
        let first_arg = j + 1;
        let comma = self.next_comma(first_arg, close);
        let title = if comma == first_arg + 1 {
            match self.kind(first_arg) {
                Some(Tk::Str) => decode(strip_ends(self.text(first_arg)), false),
                Some(Tk::Template) => decode(strip_ends(self.text(first_arg)), true),
                _ => None,
            }
        } else {
            None
        };
        if title.is_none() && annotation && self.annotation_args(first_arg, comma, close) {
            return Some((Call::Harmless, close + 1));
        }
        let body = match role {
            Role::Describe => self.callback(comma, close),
            Role::Test => Body::None,
        };
        Some((
            Call::Made {
                role,
                close,
                title,
                body,
            },
            close + 1,
        ))
    }

    /// The first `,` at the top of the arguments from `from` to `close`, or
    /// `close`.
    fn next_comma(&self, from: usize, close: usize) -> usize {
        let mut k = from;
        while k < close && !self.is_punct(k, ",") {
            k = self.skip(k);
        }
        k
    }

    /// Whether the arguments of a call of Playwright's `test.skip` (and its kin) from
    /// `first` to `close`, its first ending at `comma`, are those of an annotation,
    /// not of a test whose title is not literal: none; a callback first
    /// (`({ browserName }) => …`); or a first argument that is no string or
    /// template, then a literal description and nothing more (`isMobile, "why"`).
    /// One argument that is not a callback (`test.skip(name)`) may be a title.
    fn annotation_args(&self, first: usize, comma: usize, close: usize) -> bool {
        if first >= close {
            return true;
        }
        let mut k = first;
        let mut callback = self.is_word(first, "function")
            || (self.is_word(first, "async") && self.is_word(first + 1, "function"));
        while k < comma && !callback {
            callback = self.is_punct(k, "=>");
            k = self.skip(k);
        }
        if callback {
            return true;
        }
        if comma >= close
            || matches!(
                self.kind(first),
                Some(Tk::Str | Tk::Template | Tk::TemplateHead)
            )
        {
            return false;
        }
        let second = comma + 1;
        self.next_comma(second, close) == second + 1
            && second + 1 == close
            && matches!(self.kind(second), Some(Tk::Str | Tk::Template))
    }

    /// The body of the callback among the arguments after the title of a
    /// `describe`, from the `,` at `comma` to the `)` at `close`.
    fn callback(&self, comma: usize, close: usize) -> Body {
        if comma >= close {
            return Body::None;
        }
        let mut m = comma + 1;
        while m < close {
            if self.is_word(m, "function")
                && let Some(open) = self.function_params(m)
                && let Some(body) = self.body_after(self.t.partner(open) + 1, close)
            {
                return Body::Brace(body);
            }
            if self.is_punct(m, "=>") && self.kind(m + 1) == Some(Tk::Open(Delim::Brace)) {
                return Body::Brace(m + 1);
            }
            m = self.skip(m);
        }
        if close == comma + 1 {
            // A `,` that ends the list, and nothing after it.
            Body::None
        } else {
            Body::Unreadable
        }
    }

    /// The index of the file: the top level, and the body of every `describe` with
    /// a literal title and a readable callback, each scanned once.
    fn index(&self) -> Vec<Level> {
        let mut levels = vec![Level::default()];
        let mut queue = Queue {
            next_id: 1,
            work: vec![(0, 0, self.t.len())],
        };
        while let Some((id, lo, hi)) = queue.work.pop() {
            let mut level = Level::default();
            self.scan(lo, hi, &mut level, &mut queue);
            levels.resize_with(queue.next_id.max(levels.len()), Level::default);
            if let Some(slot) = levels.get_mut(id) {
                *slot = level;
            }
        }
        levels
    }

    /// Indexes the level of tokens `lo..hi` into `level`, with the blocks of its
    /// statements, in the order of the file: one loop over its tokens, a group not
    /// read being one; each `describe` it holds with a body is given the next id
    /// and left in `queue`.
    fn scan(&self, lo: usize, hi: usize, level: &mut Level, queue: &mut Queue) {
        let mut i = lo;
        let mut colon = usize::MAX;
        while i < hi {
            match self.kind(i) {
                Some(Tk::Open(Delim::Brace)) if self.statement_block(i, lo, colon) => i += 1,
                Some(Tk::Close(Delim::Brace)) => i += 1,
                Some(Tk::Open(Delim::Paren | Delim::Bracket))
                    if self.starts_statement(i, lo, colon) =>
                {
                    let chain = self.chain(i);
                    if chain.helper {
                        level.helpers.add(Maker {
                            what: What::Helper,
                            at: narrow(i),
                        });
                    }
                    i = chain.end.max(i + 1);
                }
                Some(Tk::Ident) => i = self.scan_word(i, (lo, hi), &mut colon, level, queue),
                _ => i = self.skip(i),
            }
        }
    }

    /// Indexes what starts with the name at `i` on a level, and gives the token
    /// after it; the level is the tokens `lo..hi`.
    fn scan_word(
        &self,
        i: usize,
        (lo, hi): (usize, usize),
        colon: &mut usize,
        level: &mut Level,
        queue: &mut Queue,
    ) -> usize {
        if self.after_dot(i) {
            return i + 1;
        }
        let word = self.text(i);
        if word == "function" {
            return self.skip_function(i, hi);
        }
        if let Some((call, end)) = self.test_call(i) {
            let at = narrow(i);
            match call {
                Call::Made {
                    role, title: None, ..
                } => level.roles[role.slot()].add(Maker {
                    what: What::NotLiteral,
                    at,
                }),
                Call::Made {
                    role: Role::Test,
                    close,
                    title: Some(title),
                    ..
                } => level
                    .tests
                    .entry(title)
                    .or_default()
                    .push((at, narrow(close))),
                Call::Made {
                    role: Role::Describe,
                    title: Some(title),
                    body,
                    ..
                } => match body {
                    Body::Brace(brace) => {
                        let id = queue.next_id;
                        queue.next_id += 1;
                        queue.work.push((id, brace + 1, self.t.partner(brace)));
                        level
                            .describes
                            .entry(title)
                            .or_default()
                            .push(Child::Level(id));
                    }
                    Body::Unreadable => level
                        .describes
                        .entry(title)
                        .or_default()
                        .push(Child::Unreadable(at)),
                    Body::None => {}
                },
                Call::Each(role) => level.roles[role.slot()].add(Maker {
                    what: What::Each,
                    at,
                }),
                Call::TooLong(role) => level.roles[role.slot()].add(Maker {
                    what: What::TooLong,
                    at,
                }),
                Call::Harmless => {}
            }
            return end;
        }
        // A keyword is looked at before the start of a statement is, which walks
        // back over `await`s: a run of them is walked once, not once each.
        let case = matches!(word, "case" | "default");
        if (KEYWORDS.contains(&word) && !case) || !self.starts_statement(i, lo, *colon) {
            return i + 1;
        }
        if case {
            *colon = self.case_colon(i + 1, hi);
            return i + 1;
        }
        if self.is_punct(i + 1, ":") {
            // A label.
            *colon = i + 1;
            return i + 2;
        }
        let chain = self.chain(i);
        if chain.helper {
            level.helpers.add(Maker {
                what: What::Helper,
                at: narrow(i),
            });
        }
        chain.end.max(i + 1)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use whyshelf_core::domain::shelf::artifact::{ArtifactState, Untold};

    use super::super::file::ScanLimits;
    use super::*;

    fn n(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    fn asks(names: &[&str]) -> SpaceAsks {
        names
            .iter()
            .map(|t| (n(t), BTreeSet::from([Comparison::Bytes])))
            .collect()
    }

    async fn write(dir: &Path, at: &str, text: &str) {
        let full = dir.join(at);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(full, text).await.unwrap();
    }

    async fn text(root: &Path, name: &str) -> Option<String> {
        JsTestSpace::new(root.to_path_buf())
            .current(&n(name))
            .await
            .unwrap()
            .map(|b| String::from_utf8(b).unwrap())
    }

    async fn reading(root: &Path, name: &str) -> ArtifactReading {
        let found = JsTestSpace::new(root.to_path_buf())
            .resolve(&asks(&[name]))
            .await
            .unwrap();
        found[&n(name)].clone()
    }

    async fn unknown_why(root: &Path, name: &str) -> String {
        match reading(root, name).await.state(&Comparison::Bytes) {
            ArtifactState::Unknown(Untold::Said(why)) => why.as_str().to_owned(),
            other => panic!("{name}: {other:?}"),
        }
    }

    /// What the test of `titles` (the describes, then the test) comes to in the
    /// source `text` of a file at `file`.
    fn in_file(file: &str, text: &str, titles: &[&str]) -> Found {
        let (test, describes) = titles.split_last().unwrap();
        let wanted = [Wanted {
            describes: describes.iter().map(|d| (*d).to_owned()).collect(),
            test: (*test).to_owned(),
        }];
        let path: RelPath = file.parse().unwrap();
        look_up(&path, text.as_bytes().to_vec(), &wanted).remove(0)
    }

    /// The same, in a `.test.ts` file.
    fn in_source(text: &str, titles: &[&str]) -> Found {
        in_file("t.test.ts", text, titles)
    }

    fn text_of(text: &str, titles: &[&str]) -> String {
        match in_source(text, titles) {
            Found::Text(t) => t,
            other => panic!("{titles:?}: {other:?}"),
        }
    }

    fn cannot_tell(found: &Found) -> String {
        match found {
            Found::CannotTell(why) => why.to_string(),
            other => panic!("{other:?}"),
        }
    }

    /// The kinds and texts of the tokens of `src`, read as a file that may hold JSX
    /// when `jsx`.
    fn tokens_as(src: &str, jsx: bool) -> Vec<(Tk, String)> {
        let lexed = Lexer::run(src, jsx).unwrap();
        (0..lexed.t.len())
            .map(|i| (lexed.t.t[i].kind, lexed.text(i).to_owned()))
            .collect()
    }

    fn tokens(src: &str) -> Vec<(Tk, String)> {
        tokens_as(src, false)
    }

    fn regexes_as(src: &str, jsx: bool) -> Vec<String> {
        tokens_as(src, jsx)
            .into_iter()
            .filter(|(k, _)| *k == Tk::Regex)
            .map(|(_, t)| t)
            .collect()
    }

    fn regexes(src: &str) -> Vec<String> {
        regexes_as(src, false)
    }

    const SOURCE: &str = r#"import { describe, it, test, expect, beforeEach } from "vitest";

// A comment at the top.
const helper = (x: number) => x + 1;

describe("basket", () => {
  beforeEach(() => {
    reset();
  });

  it("adds an item", () => {
    // A comment inside.
    expect(helper(1)).toBe(2);
  });

  it.only('keeps the order', async () => {
    expect([1, 2].map(helper)).toEqual([2, 3]);
  });

  describe(`nested`, function () {
    test.skip("is skipped", () => {});
    test.concurrent("runs at once", () => {});
    test.skipIf(process.env.CI)("unless on CI", () => {});
    it.runIf(true).concurrent("only when run", () => {});
    it.todo("is to do");
  });
});

test("Cart A \x42 \"q\"", () => {
  expect(1 / 2 / 3).toBeLessThan(1);
});

test('a \\ b', () => {});
"#;

    #[test]
    fn the_adapter_offers_the_preset_js_test_for_its_space() {
        let space = Space::new(JS_TEST_SPACE).unwrap();
        assert_eq!(
            presets(&space),
            BuiltinPresets::from([(
                PresetName::new(JS_TEST_PRESET).unwrap(),
                PresetValues {
                    space: Some("js-test".to_owned()),
                    compare: Some("bytes".to_owned()),
                    ignore: None,
                    means: Some(Propagation::Depends),
                }
            )])
        );
        assert_eq!(MAX_FILE_BYTES, ScanLimits::default().file_bytes);
        assert_eq!(std::mem::size_of::<Token>(), 12);
    }

    #[tokio::test]
    async fn a_test_found_is_there_with_the_hash_of_its_text() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        write(r, "tests/basket.test.ts", SOURCE).await;
        let present = [
            "tests/basket.test.ts::basket::adds an item",
            "tests/basket.test.ts::basket::keeps the order",
            "tests/basket.test.ts::basket::nested::is skipped",
            "tests/basket.test.ts::basket::nested::runs at once",
            "tests/basket.test.ts::basket::nested::unless on CI",
            "tests/basket.test.ts::basket::nested::only when run",
            "tests/basket.test.ts::basket::nested::is to do",
            "tests/basket.test.ts::Cart A B \"q\"",
            "tests/basket.test.ts::a \\\\ b",
        ];
        let found = JsTestSpace::new(r.to_path_buf())
            .resolve(&asks(&present))
            .await
            .unwrap();
        for name in present {
            let text = text(r, name).await.unwrap_or_else(|| panic!("{name}"));
            assert_eq!(
                found[&n(name)],
                ArtifactReading::of_bytes(
                    text.as_bytes(),
                    &BTreeSet::from([Comparison::Bytes]),
                    None
                ),
                "{name}"
            );
        }
        assert_eq!(
            text(r, "tests/basket.test.ts::basket::adds an item")
                .await
                .unwrap(),
            "it ( \"adds an item\" , ( ) => { expect ( helper ( 1 ) ) . toBe ( 2 ) ; } )"
        );
        assert_eq!(
            text(r, "tests/basket.test.ts::basket::nested::unless on CI")
                .await
                .unwrap(),
            "test . skipIf ( process . env . CI ) ( \"unless on CI\" , ( ) => { } )"
        );
        assert_eq!(
            text(r, "tests/basket.test.ts::basket::nested::is to do")
                .await
                .unwrap(),
            "it . todo ( \"is to do\" )"
        );
    }

    #[test]
    fn titles_are_read_from_strings_and_templates_with_their_escapes() {
        for (source, title) in [
            ("it('it\\'s', () => {});", "it's"),
            ("it(\"tab\\there\", () => {});", "tab\there"),
            ("it(`plain`, () => {});", "plain"),
            ("it(`a\\`b`, () => {});", "a`b"),
            (
                "it('\\u{1F600} \\uD83D\\uDE00', () => {});",
                "\u{1F600} \u{1F600}",
            ),
            ("it('\\101\\0', () => {});", "A\0"),
            ("it('line\\\ncontinued', () => {});", "linecontinued"),
            ("it('a::b', () => {});", "a::b"),
            ("it(`two\r\nlines`, () => {});", "two\nlines"),
        ] {
            assert!(
                matches!(in_source(source, &[title]), Found::Text(_)),
                "{source}"
            );
        }
        let name = TestName::parse("a.ts::a\\:\\:b::c\\\\d\\:").unwrap();
        assert_eq!(name.wanted.describes, vec!["a::b".to_owned()]);
        assert_eq!(name.wanted.test, "c\\d:");
        let name = TestName::parse("a.ts::a\\:::b").unwrap();
        assert_eq!(name.wanted.describes, vec!["a:".to_owned()]);
        let name = TestName::parse("a.ts::a:::b").unwrap();
        assert_eq!(name.wanted.describes, vec!["a".to_owned()]);
        assert_eq!(name.wanted.test, ":b");
        // A lone surrogate is no text: the title is not literal.
        assert!(matches!(
            in_source("it('\\uD83D', () => {});", &["x"]),
            Found::CannotTell(_)
        ));
    }

    #[test]
    fn the_aliases_modifiers_and_type_arguments_of_a_callee_are_read() {
        for (source, titles) in [
            ("suite('s', () => { it('x', () => {}) });", &["s", "x"][..]),
            (
                "context('s', () => { specify('x', () => {}) });",
                &["s", "x"],
            ),
            ("xdescribe('s', () => { xit('x', () => {}) });", &["s", "x"]),
            ("fdescribe('s', () => { fit('x', () => {}) });", &["s", "x"]),
            (
                "xcontext('s', () => { xspecify('x', () => {}) });",
                &["s", "x"],
            ),
            ("xtest('x', () => {});", &["x"]),
            ("test.fails('x', () => {});", &["x"]),
            ("test.failing('x', () => {});", &["x"]),
            ("it.concurrent.only('x', () => {});", &["x"]),
            ("it<Ctx>('x', ({ a }) => {});", &["x"]),
            (
                "test.extendable<{ a: Map<string, Array<number>> }>('x', () => {});",
                &["y"],
            ),
            (
                "describe<Ctx>('s', () => { it<A<B>>('x', () => {}) });",
                &["s", "x"],
            ),
        ] {
            let found = in_source(source, titles);
            if source.contains("extendable") {
                // An unknown member is no test call.
                assert_eq!(found, Found::Absent, "{source}");
            } else {
                assert!(matches!(found, Found::Text(_)), "{source}: {found:?}");
            }
        }
        assert_eq!(
            text_of("it<Ctx>('x', () => {});", &["x"]),
            "it < Ctx > ( 'x' , ( ) => { } )"
        );
    }

    #[test]
    fn playwright_describes_annotations_and_hooks_are_read() {
        let source = r#"import { test, expect } from "@playwright/test";

test.describe.configure({ mode: "serial" });
test.use({ viewport: { width: 1, height: 1 } });
test.beforeEach(async ({ page }) => { await page.goto("/"); });

test.describe("the page", () => {
  test.slow();
  test.skip(isMobile, "not on phones");
  test.skip(({ browserName }) => browserName === "webkit");
  test.setTimeout(10_000);
  test("opens", async ({ page }) => { await expect(page).toHaveTitle(/x/); });
  test.fixme("is to fix", async () => {});
  test.fail("fails", async () => {});
  test.slow("is slow", async () => {});
  test.describe.serial.only("in turn", () => {
    test("first", async () => {});
  });
  test.describe.parallel("at once", () => {
    test("one", async () => {});
  });
  test.describe.fixme("broken", () => {
    test("two", async () => {});
  });
});
"#;
        for titles in [
            &["the page", "opens"][..],
            &["the page", "is to fix"],
            &["the page", "fails"],
            &["the page", "is slow"],
            &["the page", "in turn", "first"],
            &["the page", "at once", "one"],
            &["the page", "broken", "two"],
        ] {
            assert!(
                matches!(in_source(source, titles), Found::Text(_)),
                "{titles:?}"
            );
        }
        // The hooks, the settings and the annotations make nothing.
        assert_eq!(in_source(source, &["the page", "missing"]), Found::Absent);
        assert_eq!(in_source(source, &["missing"]), Found::Absent);
        // A test of Jest's `skip` with a title that is not literal still may make one.
        let why = cannot_tell(&in_source("test.skip(name, () => {});", &["x"]));
        assert!(why.contains("`test.skip`"), "{why}");
    }

    #[test]
    fn only_playwrights_test_has_annotations_and_one_that_may_be_a_title_may_make_a_test() {
        // Annotations: no argument, a callback, a condition and a description.
        for source in [
            "test.skip();",
            "test.fixme();",
            "test.slow();",
            "test.skip(({ browserName }) => browserName === 'webkit');",
            "test.skip(async function () { return true; });",
            "test.skip(isMobile, 'not on phones');",
            "test.fixme(a && b, `broken`);",
        ] {
            assert_eq!(in_source(source, &["x"]), Found::Absent, "{source}");
        }
        // A single argument that may be a title, another callee, or a string-like
        // first argument: a test whose title is not literal.
        for (source, says) in [
            ("test.skip(name);", "`test.skip`"),
            ("test.fixme(name);", "`test.fixme`"),
            ("it.skip(name);", "`it.skip`"),
            ("it.skip(isMobile, 'why');", "`it.skip`"),
            ("xit.skip();", "`xit.skip`"),
            ("test.skip(`a ${b}`, 'why');", "`test.skip`"),
            ("test.skip(name, () => {});", "`test.skip`"),
            ("test.skip(name, 'why', 1);", "`test.skip`"),
        ] {
            let why = cannot_tell(&in_source(source, &["x"]));
            assert!(why.contains(says), "{source}: {why}");
            assert!(why.contains("whose title is not a literal"), "{why}");
        }
    }

    #[test]
    fn type_arguments_too_long_to_step_over_may_make_a_test() {
        let long = format!(
            "it<{}A>('x', () => {{}});\n",
            "A | ".repeat(MAX_TYPE_ARGUMENTS)
        );
        let why = cannot_tell(&in_source(&long, &["x"]));
        assert!(
            why.contains(
                "`it` (line 1, column 1), whose type arguments are longer than 256 tokens"
            ),
            "{why}"
        );
        let long = format!(
            "describe<{}A>('d', () => {{}});\n",
            "A | ".repeat(MAX_TYPE_ARGUMENTS)
        );
        assert!(cannot_tell(&in_source(&long, &["d", "x"])).contains("`describe`"));
        // Within the bound the call is read; with no list at all it is none.
        let within = format!("it<{}A>('x', () => {{}});\n", "A | ".repeat(100));
        assert!(matches!(in_source(&within, &["x"]), Found::Text(_)));
        assert_eq!(in_source("x = it < 3;\n", &["x"]), Found::Absent);
    }

    #[tokio::test]
    async fn an_edit_of_white_space_comments_or_another_test_keeps_the_text() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let name = "t.test.ts::basket::adds an item";
        write(r, "t.test.ts", SOURCE).await;
        let before = text(r, name).await.unwrap();

        let respaced = SOURCE.replace(
            "  it(\"adds an item\", () => {\n    // A comment inside.\n    expect(helper(1)).toBe(2);\n  });",
            "  it(  \"adds an item\",()=>{ /* another */ expect( helper(1) )\n.toBe(2)\n\n; } );",
        );
        assert_ne!(respaced, SOURCE);
        write(r, "t.test.ts", &respaced).await;
        assert_eq!(text(r, name).await.unwrap(), before);

        let commented = SOURCE.replace("// A comment at the top.", "/* Another,\n spread. */");
        write(r, "t.test.ts", &commented).await;
        assert_eq!(text(r, name).await.unwrap(), before);

        let other = SOURCE.replace("toEqual([2, 3])", "toEqual([2, 4])");
        write(r, "t.test.ts", &other).await;
        assert_eq!(text(r, name).await.unwrap(), before);
        let other = SOURCE.replace("reset();", "reset(1);");
        write(r, "t.test.ts", &other).await;
        assert_eq!(text(r, name).await.unwrap(), before);

        // Its body, its title, its modifiers.
        let body = SOURCE.replace("toBe(2)", "toBe(3)");
        write(r, "t.test.ts", &body).await;
        assert_ne!(text(r, name).await.unwrap(), before);
        let only = SOURCE.replace("it(\"adds an item\"", "it.only(\"adds an item\"");
        write(r, "t.test.ts", &only).await;
        assert_ne!(text(r, name).await.unwrap(), before);
        let quoted = SOURCE.replace("it(\"adds an item\"", "it('adds an item'");
        write(r, "t.test.ts", &quoted).await;
        assert_ne!(text(r, name).await.unwrap(), before);
        let renamed = SOURCE.replace("\"adds an item\"", "\"counts all baskets\"");
        write(r, "t.test.ts", &renamed).await;
        assert_eq!(text(r, name).await, None);
        // A string's contents are kept as written.
        let a = text_of("it('t', () => { s('a  b'); });", &["t"]);
        let b = text_of("it('t', () => { s('a b'); });", &["t"]);
        assert_ne!(a, b);
        let a = text_of("it('t', () => { s(`a  ${x}  b`); });", &["t"]);
        let b = text_of("it('t', () => { s(`a ${x} b`); });", &["t"]);
        assert_ne!(a, b);
        assert_eq!(a, "it ( 't' , ( ) => { s ( `a  ${ x }  b` ) ; } )");
    }

    #[test]
    fn a_line_end_is_kept_where_it_changes_what_the_code_does() {
        let joined = text_of("it('t', () => { return v; });", &["t"]);
        let broken = text_of("it('t', () => { return\n v; });", &["t"]);
        assert_ne!(joined, broken);
        assert_eq!(broken, "it ( 't' , ( ) => { return\nv ; } )");
        let postfix = text_of("it('t', () => { a\n++b; });", &["t"]);
        assert_ne!(postfix, text_of("it('t', () => { a ++b; });", &["t"]));
        for (one, other) in [
            ("throw e;", "throw\ne;"),
            ("break a;", "break\na;"),
            ("continue a;", "continue\na;"),
            ("yield v;", "yield\nv;"),
        ] {
            assert_ne!(
                text_of(&format!("it('t', () => {{ {one} }});"), &["t"]),
                text_of(&format!("it('t', () => {{ {other} }});"), &["t"]),
                "{other}"
            );
        }
        // Elsewhere a line end is white space.
        assert_eq!(
            text_of("it('t', () => { expect(a)\n  .toBe(b); });", &["t"]),
            text_of("it('t', () => { expect(a).toBe(b); });", &["t"])
        );
    }

    #[tokio::test]
    async fn a_test_that_is_not_there_is_absent() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        write(r, "t.test.ts", SOURCE).await;
        let absent = [
            "t.test.ts::basket::missing",
            "t.test.ts::adds an item",
            "t.test.ts::other::adds an item",
            "t.test.ts::basket::nested::adds an item",
            "t.test.ts::basket::Counts the baskets",
            "t.test.ts::Basket::adds an item",
            "gone.test.ts::basket::adds an item",
            "T.test.ts::basket::adds an item",
        ];
        let found = JsTestSpace::new(r.to_path_buf())
            .resolve(&asks(&absent))
            .await
            .unwrap();
        for name in absent {
            assert_eq!(found[&n(name)], ArtifactReading::absent(), "{name}");
            assert_eq!(text(r, name).await, None, "{name}");
        }
        assert_eq!(in_source(SOURCE, &["basket"]), Found::Absent);
        assert_eq!(
            in_source(SOURCE, &["basket", "adds an item", "x"]),
            Found::Absent
        );
    }

    #[test]
    fn twin_describes_are_each_searched_and_twin_tests_are_one() {
        let source = "if (a) { describe('d', () => { it('x', () => { 1; }); }); }\nelse { describe('d', () => { it('y', () => {}); it('x', () => { 2; }); }); }\n";
        assert_eq!(text_of(source, &["d", "y"]), "it ( 'y' , ( ) => { } )");
        assert_eq!(
            text_of(source, &["d", "x"]),
            "it ( 'x' , ( ) => { 1 ; } )\nit ( 'x' , ( ) => { 2 ; } )"
        );
        let source = "test('a', () => { 1; });\ntest.skip('a', () => { 2; });\n";
        assert_eq!(
            text_of(source, &["a"]),
            "test ( 'a' , ( ) => { 1 ; } )\ntest . skip ( 'a' , ( ) => { 2 ; } )"
        );
    }

    #[test]
    fn the_blocks_of_a_level_are_read_and_a_function_body_is_not() {
        for source in [
            "if (a) { it('a', () => {}); }",
            "if (a) {} else { it('a', () => {}); }",
            "for (const x of xs) { it('a', () => {}); }",
            "try { it('a', () => {}); } finally {}",
            "{ it('a', () => {}); }",
            "switch (k) { case \"a\": { it('a', () => {}); break; } }",
            "switch (k) { case a ? b : c: { it('a', () => {}); } default: { } }",
            "switch (k) { default: { it('a', () => {}); } }",
            "outer: { it('a', () => {}); }",
            "namespace X.Y { it('a', () => {}); }",
            "declare module \"m\" { it('a', () => {}); }",
        ] {
            assert!(
                matches!(in_source(source, &["a"]), Found::Text(_)),
                "{source}"
            );
        }
        for source in [
            "function f() { it('a', () => {}); }",
            "function f(): { done: boolean } { it('a', () => {}); }",
            "function f(): A | { x: 1 } { it('a', () => {}); }",
            "async function* g<T>(x: T): AsyncGenerator<{ a: 1 }> { it('a', () => {}); }",
            "const f = function (): { a: 1 } { it('a', () => {}); };",
            "class A { m() { it('a', () => {}); } }",
            "const f = () => { it('a', () => {}); };",
        ] {
            assert_eq!(in_source(source, &["a"]), Found::Absent, "{source}");
        }
        // After a function's body, the level goes on.
        assert!(matches!(
            in_source(
                "function f(): { done: boolean } { x(); }\nit('a', () => {});",
                &["a"]
            ),
            Found::Text(_)
        ));
        // A describe's callback may be a function, with a name or a type.
        for source in [
            "describe('d', function () { it('a', () => {}); });",
            "describe('d', function named(): void { it('a', () => {}); });",
            "describe('d', function (): { a: 1 } { it('a', () => {}); });",
            "describe('d', { timeout: 1 }, async () => { it('a', () => {}); });",
            "describe.skip.concurrent('d', (): void => { it('a', () => {}); }, 1000);",
        ] {
            assert!(
                matches!(in_source(source, &["d", "a"]), Found::Text(_)),
                "{source}"
            );
        }
    }

    #[test]
    fn a_test_not_found_where_something_may_make_it_cannot_be_told() {
        let cases = [
            (
                "it(name, () => {});\n",
                &["x"][..],
                "`it` (line 1, column 1), whose title is not a literal",
            ),
            (
                "it(`a ${b}`, () => {});\n",
                &["x"],
                "whose title is not a literal",
            ),
            (
                "it('a' + b, () => {});\n",
                &["x"],
                "whose title is not a literal",
            ),
            (
                "describe(Foo, () => {});\n",
                &["d", "x"],
                "`describe` (line 1, column 1)",
            ),
            (
                "it.each([1, 2])('n %i', (n) => {});\n",
                &["x"],
                "`it.each` (line 1, column 1)",
            ),
            (
                "test.each`a\n${1}`('n', () => {});\n",
                &["x"],
                "`test.each` (line 1",
            ),
            (
                "describe.each([1])('d', () => {});\n",
                &["d", "x"],
                "`describe.each`",
            ),
            (
                "describe('d', () => {\n  shared();\n});\n",
                &["d", "x"],
                "the call of `shared` (line 2, column 3)",
            ),
            (
                "cases.forEach((c) => it(c, () => {}));\n",
                &["x"],
                "`cases.forEach`",
            ),
            (
                "[1, 2].map((n) => test(`${n}`, () => {}));\n",
                &["x"],
                "`[…].map`",
            ),
            ("await defineTests();\n", &["x"], "`defineTests`"),
            ("x = 1\nmakeTests()\n", &["x"], "`makeTests` (line 2"),
            ("if (a) runAll();\n", &["x"], "`runAll`"),
            (
                "test.extend({})('x2', () => {});\n",
                &["x"],
                "`test.extend`",
            ),
            (
                "describe('d', makeCases);\n",
                &["d", "x"],
                "whose callback is not a function",
            ),
            (
                "for (const c of cs) { it(c.name, () => {}); }\n",
                &["x"],
                "whose title is not a literal",
            ),
            ("switch (k) { case 1: run(); }\n", &["x"], "`run`"),
        ];
        for (source, titles, says) in cases {
            let found = in_source(source, titles);
            let why = cannot_tell(&found);
            assert!(why.contains(says), "{source}: {why}");
            assert!(why.contains("may make one"), "{why}");
        }
        for (source, titles) in [
            (
                "it('a', () => { helper(); cases.forEach(c => it(c)); });\n",
                &["x"][..],
            ),
            (
                "beforeEach(() => reset());\nafterAll(cleanup);\nvi.mock('./x');\njest.setTimeout(1);\nexpect.extend({});\n",
                &["x"],
            ),
            (
                "const server = setupServer();\nlet x = make(1), y = f();\n",
                &["x"],
            ),
            (
                "function helper() { it(name, () => {}); helper2(); }\n",
                &["x"],
            ),
            ("describe(name, () => {});\n", &["x"]),
            (
                "describe('d', () => { it(name, () => {}); });\n",
                &["e", "x"],
            ),
            ("import('./x');\nexport default {};\nnew Foo();\n", &["x"]),
            ("describe.todo('d');\n", &["d", "x"]),
            ("it.each([1])('t %i', () => {});\n", &["d", "x"]),
        ] {
            assert_eq!(in_source(source, titles), Found::Absent, "{source}");
        }
        assert!(matches!(
            in_source(
                "shared();\nit(name, () => {});\nit('x', () => {});\n",
                &["x"]
            ),
            Found::Text(_)
        ));
    }

    #[test]
    fn the_reason_names_the_first_few_makers_and_counts_each_other_once() {
        let mut source = String::new();
        for i in 0..20_000 {
            source.push_str(&format!("helper{i}();\nit(name{i}, () => {{}});\n"));
        }
        let why = cannot_tell(&in_source(&source, &["x"]));
        assert!(why.len() < 700, "{} bytes: {why}", why.len());
        assert!(why.ends_with("; and 39995 more"), "{why}");
        assert!(!why.contains(",;") && !why.contains(",,"), "{why}");
        assert_eq!(why.matches("; ").count(), 5, "{why}");
        // Five on one line are five, each with its column, and the rest counted.
        let why = cannot_tell(&in_source("f(); f(); f(); f(); f(); f(); f();\n", &["x"]));
        assert!(why.contains("(line 1, column 6)"), "{why}");
        assert!(why.ends_with("; and 2 more"), "{why}");
        // Across the levels of the way, too.
        let source = "a();\ndescribe('d', () => { b(); c(); });\n";
        let why = cannot_tell(&in_source(source, &["d", "x"]));
        assert!(
            why.contains("`a`") && why.contains("`b`") && why.contains("`c`"),
            "{why}"
        );
        assert!(!why.contains("more"), "{why}");
        let long = format!("{}();\n", "a".repeat(10_000));
        let why = cannot_tell(&in_source(&long, &["x"]));
        assert!(why.len() < 300, "{why}");
    }

    #[test]
    fn the_regex_and_the_division_are_told_by_the_token_before() {
        let division = |src: &str| {
            let t = tokens(src);
            assert!(t.iter().all(|(k, _)| *k != Tk::Regex), "{src}: {t:?}");
            t.iter()
                .filter(|(k, s)| *k == Tk::Punct && s == "/")
                .count()
        };
        assert_eq!(division("a / b / c"), 2);
        assert_eq!(division("f(x) / 2 / g"), 2);
        assert_eq!(division("(a) / (b) / c"), 2);
        assert_eq!(division("x = {} / 2 / 1"), 2);
        assert_eq!(division("a[0] / 2 / 1"), 2);
        assert_eq!(division("x++ / 2 / 1"), 2);
        assert_eq!(division("a.return / 2 / 1"), 2);
        assert_eq!(division("this / 2 / 1"), 2);
        assert_eq!(division("1 / 2 / 3"), 2);
        assert_eq!(division("'s' / 2 / 1"), 2);
        assert_eq!(division("x /= 2 / 1"), 1);
        // TypeScript's non-null, and `of` as a name.
        assert_eq!(division("total! / 2 / n"), 2);
        assert_eq!(division("f(x)! / 2 / n"), 2);
        assert_eq!(division("a[0]! / 2 / n"), 2);
        assert_eq!(division("const of = 4; of / 2 / 1"), 2);
        assert_eq!(regexes("x = ! /re/.test(s)"), ["/re/"]);
        assert_eq!(regexes("for (const x of /a/.exec(s)) {}"), ["/a/"]);
        assert_eq!(regexes("x = /re/g"), ["/re/g"]);
        assert_eq!(regexes("if (a) /re/.test(s)"), ["/re/"]);
        assert_eq!(regexes("while (a) /a\\/b/.exec(s)"), ["/a\\/b/"]);
        assert_eq!(regexes("function f() {}\n/re/.test(s)"), ["/re/"]);
        assert_eq!(regexes("{}\n/re/i.test(s)"), ["/re/i"]);
        assert_eq!(regexes("return /re/"), ["/re/"]);
        assert_eq!(regexes("f(/[/]/, typeof /x/)"), ["/[/]/", "/x/"]);
        assert_eq!(regexes("a = b ? /x/ : /y/"), ["/x/", "/y/"]);
        assert_eq!(regexes("`${/x/}`"), ["/x/"]);
        assert_eq!(regexes("/=/.test(s)"), ["/=/"]);
        // The body of a class, an interface or an enum is a block.
        assert_eq!(
            regexes("class A extends Base<T> {}\n/re/.test(s)"),
            ["/re/"]
        );
        assert_eq!(
            regexes("class A<T> implements I<T> { m() {} }\n/re/.test(s)"),
            ["/re/"]
        );
        assert_eq!(regexes("enum E { A }\n/re/.test(s)"), ["/re/"]);
        assert_eq!(regexes("x = /* c */ /re/"), ["/re/"]);
        assert_eq!(division("a /* c */ / b"), 1);
    }

    #[test]
    fn jsx_is_read_in_the_files_that_may_hold_it() {
        let in_jsx = |src: &str| tokens_as(src, true);
        // A closing tag, a spread, text with a quote, a slash or `//`.
        for src in [
            "render(<div>hi</div>)",
            "render(<Wrap {...props} />)",
            "render(<Wrap {...props}/>)",
            "render(<a b={c} d=\"e\"/>)",
            "render(<p>don't</p>)",
            "render(<p>/home</p>)",
            "render(<p>see // not a comment</p>)",
            "render(<><A /><B>{x / 2}</B></>)",
            "render(<a title=\"two\nlines\">{`t${1}`}</a>)",
            "const f = <T,>(x: T) => x / 2;",
            "const g = <T extends unknown>(x: T) => x / 2;",
        ] {
            let t = in_jsx(src);
            assert!(t.iter().all(|(k, _)| *k != Tk::Regex), "{src}: {t:?}");
        }
        let t = in_jsx("render(<p> don't </p>)");
        assert!(t.contains(&(Tk::JsxText, "don't".to_owned())), "{t:?}");
        assert_eq!(regexes_as("render(<a>{/re/.test(s)}</a>)", true), ["/re/"]);
        assert!(matches!(
            in_file(
                "t.test.tsx",
                "it('x', () => { render(<Wrap {...props} />); expect(a / b).toBe(1); });",
                &["x"]
            ),
            Found::Text(_)
        ));
        assert!(matches!(
            in_file(
                "t.test.jsx",
                "it('x', () => { render(<p>don't</p>); });",
                &["x"]
            ),
            Found::Text(_)
        ));
        // In TypeScript's own files `<T>x` is a cast, not a tag.
        assert!(matches!(
            in_file("t.test.ts", "const a = <any>b;\nit('x', () => {});", &["x"]),
            Found::Text(_)
        ));
        // An element not closed does not lex.
        let why = cannot_tell(&in_file("t.test.tsx", "x = <p>open\n", &["x"]));
        assert!(why.contains("an element of JSX is not closed"), "{why}");
    }

    #[test]
    fn the_lexer_reads_each_kind_of_token() {
        let t = tokens(
            "#!/usr/bin/env node\na?.b ?? c?.5:1; x >>>= 0x1F + 1.5e-3 + 10n; #p; `a${`b${c}`}d`; @dec",
        );
        let texts: Vec<&str> = t.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(
            texts,
            [
                "a", "?.", "b", "??", "c", "?", ".5", ":", "1", ";", "x", ">>>=", "0x1F", "+",
                "1.5e-3", "+", "10n", ";", "#p", ";", "`a${", "`b${", "c", "}`", "}d`", ";", "@",
                "dec"
            ]
        );
        assert_eq!(t[20].0, Tk::TemplateHead);
        assert_eq!(t[23].0, Tk::TemplateTail);
        assert_eq!(t[24].0, Tk::TemplateTail);
        let t = tokens("`${ {a: 1}.a }`");
        assert_eq!(t.len(), 9, "{t:?}");
        assert_eq!(t[8], (Tk::TemplateTail, "}`".to_owned()));
    }

    #[tokio::test]
    async fn a_file_that_cannot_be_read_as_a_script_cannot_be_told() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        for (source, says) in [
            (
                "it('x', () => {\n",
                "a `{` is not closed (at line 1, column 15)",
            ),
            (
                "it('x', () => {});\n'open\n",
                "a string is not closed on its line (at line 2, column 1)",
            ),
            (
                "it('x', () => {});\n`open ${\n",
                "a template literal is not closed (at line 2, column 1)",
            ),
            ("x = /open\n", "a regular expression is not closed"),
            ("/* open\n", "a block comment is not closed"),
            ("it('x', () => {});\n)", "a `)` closes nothing"),
            ("(]", "a `]` closes another kind of bracket"),
            ("a \u{7} b", "the character '\\u{7}' starts no token"),
        ] {
            write(r, "bad.test.ts", source).await;
            let why = unknown_why(r, "bad.test.ts::x").await;
            assert!(
                why.starts_with("bad.test.ts does not lex as JavaScript or TypeScript"),
                "{why}"
            );
            assert!(why.contains(says), "{source:?}: {why}");
        }
        tokio::fs::write(r.join("bytes.test.ts"), b"it('x', () => {});\xff")
            .await
            .unwrap();
        let why = unknown_why(r, "bytes.test.ts::x").await;
        assert!(why.contains("not UTF-8"), "{why}");
        for (open, close) in [("(", ")"), ("[", "]"), ("{", "}"), ("`${", "}`")] {
            let deep = format!(
                "it('x', () => {{}});\nx = {}1{};\n",
                open.repeat(200),
                close.repeat(200)
            );
            write(r, "deep.test.ts", &deep).await;
            let why = unknown_why(r, "deep.test.ts::x").await;
            assert!(why.contains("deeper than 128"), "{open}: {why}");
        }
        let deep = format!(
            "it('x', () => {{}});\nx = {}{};\n",
            "<a>".repeat(200),
            "</a>".repeat(200)
        );
        write(r, "deep.test.tsx", &deep).await;
        let why = unknown_why(r, "deep.test.tsx::x").await;
        assert!(why.contains("deeper than 128"), "{why}");
        let within = format!(
            "it('x', () => {{ y = {}1{}; }});\n",
            "(".repeat(120),
            ")".repeat(120)
        );
        write(r, "within.test.ts", &within).await;
        assert!(reading(r, "within.test.ts::x").await.is_present());
        let small = JsTestSpace {
            max_file: 100,
            ..JsTestSpace::new(r.to_path_buf())
        };
        write(r, "big.test.ts", SOURCE).await;
        let found = small.resolve(&asks(&["big.test.ts::a"])).await.unwrap();
        match found[&n("big.test.ts::a")].state(&Comparison::Bytes) {
            ArtifactState::Unknown(Untold::Said(why)) => {
                assert!(why.as_str().contains("larger than 100 bytes"), "{why}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(small.current(&n("big.test.ts::a")).await.unwrap(), None);
    }

    /// Sources that nest or run long, each answered without a crash: nothing
    /// recurses on them, and nothing costs the square of their length.
    #[tokio::test]
    async fn a_hostile_file_is_answered_without_a_crash() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let tail = "\nit('a', () => {});\n";
        let hostile = [
            format!("x = {}1;{tail}", "-".repeat(200_000)),
            format!("x = {}1;{tail}", "!".repeat(200_000)),
            format!("x = {}1;{tail}", "a => ".repeat(100_000)),
            format!("{}x;{tail}", "await ".repeat(100_000)),
            format!("{}x;{tail}", "void ".repeat(100_000)),
            format!("x = 1{};{tail}", " / 2".repeat(100_000)),
            format!("x = 1{};{tail}", " + a.b".repeat(100_000)),
            format!("x = '{}';{tail}", "s".repeat(4_000_000)),
            format!("x = `{}`;{tail}", "t\n".repeat(2_000_000)),
            format!("x = {}1{};{tail}", "`${".repeat(100), "}`".repeat(100)),
            format!("{}{tail}", "f();".repeat(200_000)),
            format!("{}{tail}", "{}".repeat(200_000)),
            format!("{}{tail}", "x\n".repeat(200_000)),
            format!("x = /{}/;{tail}", "[/]".repeat(100_000)),
            format!("x = {};{tail}", "it < ".repeat(100_000)),
            format!("{}{tail}", "case ".repeat(100_000)),
            format!("{};{tail}", "namespace a.b ".repeat(50_000)),
            format!("{}{{}}{tail}", "a.".repeat(100_000)),
            format!("{}{tail}", "function f(): ".repeat(50_000)),
        ];
        for (i, source) in hostile.iter().enumerate() {
            let file = format!("h{i}.test.ts");
            write(r, &file, source).await;
            let started = std::time::Instant::now();
            assert!(reading(r, &format!("{file}::a")).await.is_present(), "{i}");
            let other = reading(r, &format!("{file}::b")).await;
            assert!(!other.is_present(), "{i}");
            assert!(
                started.elapsed().as_secs() < 20,
                "{i}: {:?}",
                started.elapsed()
            );
        }
    }

    /// Many names of one file cost the file once, then each name's way.
    #[tokio::test]
    async fn many_names_of_one_file_are_found_in_one_index() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let mut source = String::from("describe('d', () => {\n");
        for i in 0..4_000 {
            source.push_str(&format!(
                "  it('t{i}', () => {{ expect({i}).toBe({i}); }});\n"
            ));
        }
        source.push_str("});\n");
        write(r, "many.test.ts", &source).await;
        let names: Vec<String> = (0..4_000)
            .map(|i| format!("many.test.ts::d::t{i}"))
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let started = std::time::Instant::now();
        let found = JsTestSpace::new(r.to_path_buf())
            .resolve(&asks(&names))
            .await
            .unwrap();
        assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
        assert!(names.iter().all(|name| found[&n(name)].is_present()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_file_through_a_link_out_of_the_root_cannot_be_told() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "a.test.ts", SOURCE).await;
        tokio::fs::symlink(outside.path(), root.path().join("out"))
            .await
            .unwrap();
        let why = unknown_why(root.path(), "out/a.test.ts::a \\\\ b").await;
        assert!(why.contains("leads outside the root"), "{why}");
        write(root.path(), "src/a.test.ts", SOURCE).await;
        tokio::fs::symlink(root.path().join("src"), root.path().join("in"))
            .await
            .unwrap();
        assert!(
            reading(root.path(), "in/a.test.ts::a \\\\ b")
                .await
                .is_present()
        );
    }

    #[tokio::test]
    async fn a_text_out_of_the_grammar_is_not_a_name() {
        let nowhere = tempfile::tempdir().unwrap();
        let wrongs = [
            ("a.test.ts", "no `::` follows"),
            ("a.test::x", "does not end in `.ts`"),
            ("a.test.rs::x", "does not end in `.ts`"),
            ("a.test.TS::x", "does not end in `.ts`"),
            ("/a.test.ts::x", "no path under the root"),
            ("src/../a.test.ts::x", "no path under the root"),
            ("./a.test.ts::x", "no path under the root"),
            ("src\\a.test.ts::x", "no path under the root"),
            ("a.test.ts::", "empty"),
            ("a.test.ts::d::", "empty"),
            ("a.test.ts::::x", "empty"),
            ("a.test.ts::a\\b", "holds `\\b`"),
            ("a.test.ts::a\\", "holds `\\`"),
            ("a.test.ts::a\\n", "holds `\\n`"),
        ];
        let names: Vec<&str> = wrongs.iter().map(|(w, _)| *w).collect();
        let source = JsTestSpace::new(nowhere.path().to_path_buf());
        let found = source.resolve(&asks(&names)).await.unwrap();
        for (wrong, says) in wrongs {
            let why = found[&n(wrong)].not_a_name_why().map(|w| w.as_str());
            assert!(
                why.is_some_and(|w| w.starts_with("no test name") && w.contains(says)),
                "{wrong}: {why:?}"
            );
            assert_eq!(source.current(&n(wrong)).await.unwrap(), None);
        }
        for ext in EXTENSIONS {
            assert!(TestName::parse(&format!("a{ext}::x")).is_ok(), "{ext}");
        }
    }

    #[test]
    fn a_hashbang_and_a_mark_of_order_are_skipped() {
        for source in [
            "#!/usr/bin/env node\nit('a', () => {});\n",
            "\u{feff}it('a', () => {});\n",
        ] {
            assert_eq!(
                in_source(source, &["a"]),
                Found::Text("it ( 'a' , ( ) => { } )".to_owned()),
                "{source:?}"
            );
        }
    }

    #[test]
    fn a_construct_swallows_no_next_test() {
        for before in [
            "describe.each([1])('d', () => {})",
            "it.each`a`('t', () => {})",
            "helper()",
            "helper\n",
            "it.only",
            "const test = 1",
            "function test() {}",
            "function f(): { a: 1 }",
            "x.test('a')",
            "/re/.test('a')",
            "class A { test() {} }",
            "describe('d')",
            "describe('d',)",
            "it(`t`)",
            "if (a) b\n",
            "test.skipIf(a)",
            "it<A",
            "switch (k) { case 1: }",
            "label: x",
        ] {
            let source = format!("{before};\nit('a', () => {{}});\n");
            assert_eq!(
                text_of(&source, &["a"]),
                "it ( 'a' , ( ) => { } )",
                "{before}"
            );
        }
    }
}
