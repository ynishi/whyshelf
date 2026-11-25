//! This repository's own verifications name their tests as DEC-112 and DEC-56 say.
//!
//! - The book `verification` has one link with the preset `test` (`cases`), of the
//!   space `rust-test`; when it declares a cover, its strength is `error`, it takes
//!   the parts of a listing whole through `tests`, and it writes no exclude.
//! - Over the files the include of that cover takes, a verification names tests by
//!   function under `cases`, and names a file whole under `tests` only when rust-test
//!   lists no test of it one by one (none in it, or a part of the listing); a name
//!   under `cases` is a test of a file the include takes, which rust-test lists whole
//!   and holds the name. The file of a test is where rust-test places it, in one file.
//! - A verification over a file of the end-to-end tests that uses the helpers of a
//!   directory (`mod <dir>;`, with `crates/whyshelf-cli/tests/<dir>/mod.rs`) names
//!   every file of that directory under `tests`; such a helper holds no test nor
//!   anything that may make one (DEC-56).
//! - No file under `crates/whyshelf-cli/tests/` but those helpers is named, whole or
//!   by its tests, by more than two verifications, so an edit of one feature's tests
//!   leaves the approvals of the others fresh (DEC-56).
//!
//! It takes the declaration and the names of the verifications' links from the
//! application (map, book_links), read once at the side after as a check reads them,
//! reads the files of the crates through the port at the same side (the space file's
//! list and current, and where the spaces place a name), and reads the file system
//! itself nowhere else; the rule tests write only temporary projects of their own. It
//! reads no front matter and no grammar of the names: a name's file is what its space
//! places it in, and whether a file's tests are listed whole is rust-test's answer.
//! Each region (the include of the cover, and the files of the end-to-end tests when a
//! helper lies outside it) is listed once, and a file is read in the listing of its
//! region: a listing that is not whole tells nothing of any file of it.
//!
//! What it does not see:
//!
//! - the helpers of other crates and of `#[cfg(test)]` modules, and the files a test
//!   reads whole (`include!`, `include_str!`, `include_bytes!`): their naming is not
//!   asked, so an edit of one makes stale no verification that does not name it;
//! - a `mod` that a macro of an end-to-end file writes when it is expanded, and a
//!   `mod <name>;` inside an inline module (`mod m { mod common; }`): only the tokens
//!   written at the top of the file are read.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use whyshelf_core::app::shelf::state::Side;
use whyshelf_core::app::shelf::{ArtifactSource, MapReport, ShelfApp, ShelfRead};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactKey, ArtifactName, Completeness, ListedNames, ListingPart, Listings, Selection, Space,
};
use whyshelf_core::domain::shelf::map::{MapTarget, ShelfMap};
use whyshelf_core::domain::shelf::placement::Placement;
use whyshelf_core::domain::shelf::schema::{ArtifactCover, BookName, CoverSeverity, EntryName};
use whyshelf_infra::shelf::Project;
use whyshelf_infra::shelf::artifact::file::FILE_SPACE;
use whyshelf_infra::shelf::artifact::rust_test::{RUST_TEST_SPACE, TEST_PRESET};

/// The decision whose rule this guard holds the books to.
const DEC: &str = "DEC-112";

/// The book of the verifications.
const VERIFICATION: &str = "verification";

/// The link that names evidence whole, and through which a cover takes its parts.
const TESTS: &str = "tests";

/// The directory of the end-to-end tests.
const E2E: &str = "crates/whyshelf-cli/tests/";

/// The files of the crates.
const CRATES: &str = "crates/**/*.rs";

/// The files of Rust of the end-to-end tests, where their helpers are.
const E2E_RUST: &str = "crates/whyshelf-cli/tests/**/*.rs";

/// The most verifications that may name one file of the end-to-end tests.
const MOST: usize = 2;

type App = ShelfApp<Project>;

/// The link with the preset `test`, and its cover.
struct TestLink {
    name: EntryName,
    cover: Option<ArtifactCover>,
}

/// What one verification names, by link.
#[derive(Default)]
struct Named {
    tests: Vec<ArtifactKey>,
    cases: Vec<ArtifactKey>,
}

/// What rust-test's listing of a region says of the tests of one file of it: none,
/// the file is a part of the listing (with why), tests listed one by one, or the
/// listing cannot tell (cut at a bound, or not made). [`grouped`] sorts the names and
/// the parts of the listing to their files, and [`keep_in`] is the one place the four
/// are told apart.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Keep {
    NoTests,
    Part(String),
    Split(BTreeSet<ArtifactName>),
    CannotTell(Completeness),
}

/// What rust-test lists of the files of one region, by file: the names it lists one
/// by one, and why each part is not listed whole.
#[derive(Debug, Default)]
struct Tests {
    names: BTreeMap<RelPath, BTreeSet<ArtifactName>>,
    parts: BTreeMap<RelPath, Vec<String>>,
}

/// The `Keep` of `file` in the one listing of its region, read as the domain reads a
/// listing with its parts (`ListedNames::names_and_parts`): what is a part is the
/// listing's answer, and a listing that is not whole tells nothing of any file.
fn keep_in(tests: &Result<Tests, Completeness>, file: &RelPath) -> Keep {
    let tests = match tests {
        Err(completeness) => return Keep::CannotTell(completeness.clone()),
        Ok(tests) => tests,
    };
    if let Some(whys) = tests.parts.get(file) {
        Keep::Part(whys.join("; "))
    } else if let Some(names) = tests.names.get(file) {
        Keep::Split(names.clone())
    } else {
        Keep::NoTests
    }
}

/// The names of a listing by the file each is placed in (`placed`, through
/// [`file_of`]), and its parts by the file of their key among `files`, with a problem
/// for a name or a part of no file of `files`: it is not met unless the tree changes
/// while it is read, for both spaces place a file's name in that file alone.
fn grouped(
    placed: &BTreeMap<ArtifactKey, RelPath>,
    parts: &[ListingPart],
    files: &BTreeMap<RelPath, ArtifactKey>,
) -> (Tests, Vec<String>) {
    let mut tests = Tests::default();
    let mut problems = Vec::new();
    for (key, file) in placed {
        if files.contains_key(file) {
            tests
                .names
                .entry(file.clone())
                .or_default()
                .insert(key.name().clone());
        } else {
            problems.push(format!(
                "{} places {} in {file}, which the selection of its listing does not take ({DEC})",
                key.space(),
                key.name()
            ));
        }
    }
    let by_key: BTreeMap<&ArtifactKey, &RelPath> =
        files.iter().map(|(file, key)| (key, file)).collect();
    for part in parts {
        match by_key.get(part.key()) {
            Some(file) => tests
                .parts
                .entry((*file).clone())
                .or_default()
                .push(part.why().as_str().to_owned()),
            None => problems.push(format!(
                "rust-test gives the part {}, which is no file its selection takes ({DEC})",
                part.key()
            )),
        }
    }
    (tests, problems)
}

fn space(text: &str) -> Space {
    Space::new(text).expect("a space")
}

fn selection_of(include: Vec<String>) -> Result<Selection, String> {
    Selection::new(include, Vec::new()).map_err(|e| format!("no selection: {e}"))
}

/// The listing of `selection` in `space`, as `ListedNames` reads the answer.
async fn listing(app: &App, space: &Space, selection: &Selection) -> Result<ListedNames, String> {
    let listing = app
        .ports()
        .artifacts()
        .list(Side::After, space, selection)
        .await
        .map_err(|e| format!("the space {space} cannot be asked to list: {e}"))?;
    let listings = Listings::Asked(BTreeMap::from([(
        (space.clone(), selection.clone()),
        listing,
    )]));
    Ok(ListedNames::of(&listings))
}

/// The names `selection` takes in `space` when they are all listed, or how complete
/// the listing is.
async fn listed(
    app: &App,
    space: &Space,
    selection: &Selection,
) -> Result<Result<BTreeSet<ArtifactName>, Completeness>, String> {
    Ok(listing(app, space, selection)
        .await?
        .names(space, selection)
        .cloned())
}

/// The declaration, as `map` reads it.
async fn mapped(app: &App) -> Result<ShelfMap, String> {
    match app.map().await {
        Ok(MapReport::Mapped(map)) => Ok(map),
        Ok(MapReport::SchemaInvalid(problems)) => Err(format!(
            "the declaration has problems: {}",
            problems
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        )),
        Err(e) => Err(format!("the declaration cannot be read: {e}")),
    }
}

/// The one link of the book verification with the preset `test`, and the form of its
/// cover (DEC-112).
fn test_link(map: &ShelfMap) -> Result<TestLink, Vec<String>> {
    let found: Vec<(&EntryName, &Space, &Option<ArtifactCover>)> = map
        .links
        .iter()
        .filter(|link| link.from.as_str() == VERIFICATION)
        .filter_map(|link| match &link.target {
            MapTarget::Artifact {
                space,
                preset,
                cover,
                ..
            } if preset
                .as_ref()
                .is_some_and(|p| p.name().as_str() == TEST_PRESET) =>
            {
                Some((&link.name, space, cover))
            }
            _ => None,
        })
        .collect();
    let (name, space, cover) = match found.as_slice() {
        [] => {
            return Err(vec![format!(
                "the book verification has no link with the preset test ({DEC})"
            )]);
        }
        [one] => *one,
        many => {
            return Err(vec![format!(
                "the book verification has {} links with the preset test, {}: one is expected ({DEC})",
                many.len(),
                many.iter()
                    .map(|(name, _, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )]);
        }
    };
    let mut problems = Vec::new();
    if space.as_str() != RUST_TEST_SPACE {
        problems.push(format!(
            "the link with the preset test is of the space {space}, not rust-test ({DEC})"
        ));
    }
    if let Some(cover) = cover {
        if cover.severity() != CoverSeverity::Error {
            problems.push(format!(
                "the cover of {name} declares a strength that is not error ({DEC})"
            ));
        }
        if cover.parts_whole().map(EntryName::as_str) != Some(TESTS) {
            problems.push(format!(
                "the cover of {name} does not take its parts whole through tests ({DEC})"
            ));
        }
        if !cover.selection().exclude().is_empty() {
            problems.push(format!(
                "the cover of {name} writes an exclude: a file whose tests cannot all be listed is a part, taken whole through tests ({DEC})"
            ));
        }
    }
    if problems.is_empty() {
        Ok(TestLink {
            name: name.clone(),
            cover: cover.clone(),
        })
    } else {
        Err(problems)
    }
}

/// What each valid verification names under `tests` and under the link with the
/// preset `test`, by its key, from `book_links`; every file of the book that is no
/// valid document fails it.
async fn links_of(app: &App, link: &EntryName) -> Result<BTreeMap<String, Named>, Vec<String>> {
    let book = BookName::new(VERIFICATION).expect("a book");
    let report = app
        .book_links(&book)
        .await
        .map_err(|e| vec![format!("the verifications cannot be read: {e}")])?;
    if !report.not_valid.is_empty() {
        return Err(report
            .not_valid
            .iter()
            .map(|file| {
                format!(
                    "{file} is no valid document of the book verification: whyshelf check says why ({DEC})"
                )
            })
            .collect());
    }
    Ok(report
        .docs
        .into_iter()
        .map(|doc| {
            let mut links = doc.links;
            let named = Named {
                tests: links
                    .remove(&EntryName::new(TESTS).expect("a name"))
                    .unwrap_or_default(),
                cases: links.remove(link).unwrap_or_default(),
            };
            (doc.key.form.to_string(), named)
        })
        .collect())
}

/// The one file `placement` puts `key` in, or why the guard cannot read it as one:
/// a test file of this repository is reached through no link (DEC-112).
fn file_of(key: &ArtifactKey, placement: &Placement) -> Result<RelPath, String> {
    match placement {
        Placement::Only(files) if files.as_set().len() == 1 => {
            Ok(files.as_set().iter().next().expect("one").clone())
        }
        Placement::Only(files) | Placement::Partly(files) => Err(format!(
            "{} places {} in {}: a test file of this repository is reached through no link, so the guard reads a test as of one file ({DEC})",
            key.space(),
            key.name(),
            files
                .as_set()
                .iter()
                .map(RelPath::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Placement::Unplaced(why) => Err(format!(
            "{} places {} nowhere: {why} ({DEC})",
            key.space(),
            key.name()
        )),
    }
}

/// The file each of `keys` lives in, asked of the spaces at once, with a problem for
/// each the guard cannot read as of one file.
async fn placed(
    app: &App,
    keys: &BTreeSet<ArtifactKey>,
) -> Result<(BTreeMap<ArtifactKey, RelPath>, Vec<String>), Vec<String>> {
    let answer = app
        .ports()
        .artifacts()
        .places(Side::After, keys)
        .await
        .map_err(|e| vec![format!("where the names live cannot be asked: {e}")])?;
    let mut files = BTreeMap::new();
    let mut problems = Vec::new();
    for key in keys {
        match answer.get(key).map(|placement| file_of(key, placement)) {
            Some(Ok(file)) => {
                files.insert(key.clone(), file);
            }
            Some(Err(why)) => problems.push(why),
            None => problems.push(format!(
                "{} gave no place for {} ({DEC})",
                key.space(),
                key.name()
            )),
        }
    }
    Ok((files, problems))
}

/// The files of the space file a selection takes, each by its key and the file its
/// name places it in.
async fn files_taken(
    app: &App,
    selection: &Selection,
    cannot: impl Fn(&Completeness) -> String,
) -> Result<BTreeMap<RelPath, ArtifactKey>, Vec<String>> {
    let file = space(FILE_SPACE);
    let names = listed(app, &file, selection)
        .await
        .map_err(|e| vec![e])?
        .map_err(|c| vec![cannot(&c)])?;
    let keys: BTreeSet<ArtifactKey> = names
        .into_iter()
        .map(|name| ArtifactKey::new(file.clone(), name))
        .collect();
    let (placed, problems) = placed(app, &keys).await?;
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(placed.into_iter().map(|(key, path)| (path, key)).collect())
}

/// The files of the crates.
async fn rust_files(app: &App) -> Result<BTreeMap<RelPath, ArtifactKey>, Vec<String>> {
    let selection = selection_of(vec![CRATES.to_owned()]).map_err(|e| vec![e])?;
    files_taken(app, &selection, |c| {
        format!("the files of the crates cannot all be listed: {c}")
    })
    .await
}

/// A selection read once: the files of the space file it takes, each by its key, and
/// what rust-test's one listing of it says of their tests.
struct Region {
    files: BTreeMap<RelPath, ArtifactKey>,
    tests: Result<Tests, Completeness>,
}

/// The region of `include`: the space file lists it and places its names, and
/// rust-test lists it once, read with its parts, its names placed at once.
async fn region(
    app: &App,
    include: Vec<String>,
    cannot: impl Fn(&Completeness) -> String,
) -> Result<Region, Vec<String>> {
    let selection = selection_of(include).map_err(|e| vec![e])?;
    let files = files_taken(app, &selection, cannot).await?;
    let rust_test = space(RUST_TEST_SPACE);
    let read = listing(app, &rust_test, &selection)
        .await
        .map_err(|e| vec![e])?;
    let tests = match read.names_and_parts(&rust_test, &selection) {
        Err(completeness) => Err(completeness),
        Ok((names, parts)) => {
            let keys: BTreeSet<ArtifactKey> = names
                .iter()
                .map(|name| ArtifactKey::new(rust_test.clone(), name.clone()))
                .collect();
            let (at, mut problems) = placed(app, &keys).await?;
            let (tests, more) = grouped(&at, parts, &files);
            problems.extend(more);
            if !problems.is_empty() {
                return Err(problems);
            }
            Ok(tests)
        }
    };
    Ok(Region { files, tests })
}

/// The region of the include of the cover of `link`: none without a cover.
async fn include_region(app: &App, link: &TestLink) -> Result<Option<Region>, Vec<String>> {
    let Some(cover) = &link.cover else {
        return Ok(None);
    };
    let name = &link.name;
    region(app, cover.selection().include().to_vec(), |c| {
        format!("the files the include of {name} takes cannot all be listed: {c}")
    })
    .await
    .map(Some)
}

/// The text of `file` at the side after, through the port.
async fn text_of(app: &App, key: &ArtifactKey, file: &RelPath) -> Result<String, String> {
    match app.ports().artifacts().current(Side::After, key).await {
        Ok(Some(bytes)) => String::from_utf8(bytes).map_err(|_| format!("{file} is not UTF-8")),
        Ok(None) => Err(format!(
            "{file} cannot be read through the state (nothing there, past the bound of a file, or a link out of the root)"
        )),
        Err(e) => Err(format!("{file} cannot be read: {e}")),
    }
}

/// The tokens of the top of a file of Rust.
fn tokens_of(file: &RelPath, text: &str) -> Result<Vec<TokenTree>, String> {
    text.parse::<TokenStream>()
        .map(|stream| stream.into_iter().collect())
        .map_err(|e| format!("{file} cannot be read as Rust tokens: {e}"))
}

/// A module a file declares out of line at its top: `mod <name>;`.
#[derive(Debug, PartialEq, Eq)]
struct ModDecl {
    name: String,
    line: usize,
    /// Whether a `#[path …]` attribute reads it from a file of its choosing.
    path: bool,
}

/// The modules declared out of line at the top of a file (`mod <name>;`, after `pub`
/// or `pub(…)` and attributes).
fn mods(tokens: &[TokenTree]) -> Vec<ModDecl> {
    let is_ident = |at: usize, text: &str| matches!(tokens.get(at), Some(TokenTree::Ident(ident)) if ident == text);
    let mut found = Vec::new();
    for at in 0..tokens.len() {
        if !is_ident(at, "mod") {
            continue;
        }
        let (Some(TokenTree::Ident(name)), Some(TokenTree::Punct(end))) =
            (tokens.get(at + 1), tokens.get(at + 2))
        else {
            continue;
        };
        if end.as_char() != ';' {
            continue;
        }
        // Back over `pub` or `pub(…)`, then over the attributes.
        let mut before = at;
        if before >= 2
            && matches!(&tokens[before - 1], TokenTree::Group(g) if g.delimiter() == Delimiter::Parenthesis)
            && is_ident(before - 2, "pub")
        {
            before -= 2;
        } else if before >= 1 && is_ident(before - 1, "pub") {
            before -= 1;
        }
        let mut path = false;
        while before >= 2 {
            let (TokenTree::Punct(hash), TokenTree::Group(group)) =
                (&tokens[before - 2], &tokens[before - 1])
            else {
                break;
            };
            if hash.as_char() != '#' || group.delimiter() != Delimiter::Bracket {
                break;
            }
            path |= matches!(group.stream().into_iter().next(), Some(TokenTree::Ident(i)) if i == "path");
            before -= 2;
        }
        found.push(ModDecl {
            name: name.to_string(),
            line: tokens[at].span().start().line,
            path,
        });
    }
    found
}

/// The helpers of the end-to-end tests: each directory under `crates/whyshelf-cli/tests/`
/// whose `mod.rs` the state lists, with every `.rs` file under it.
fn cli_helpers(files: &BTreeMap<RelPath, ArtifactKey>) -> BTreeMap<String, BTreeSet<RelPath>> {
    let dirs: BTreeSet<String> = files
        .keys()
        .filter_map(|path| path.as_str().strip_prefix(E2E))
        .filter_map(|rest| rest.strip_suffix("/mod.rs"))
        .filter(|dir| !dir.contains('/'))
        .map(str::to_owned)
        .collect();
    dirs.into_iter()
        .map(|dir| {
            let under = format!("{E2E}{dir}/");
            let of: BTreeSet<RelPath> = files
                .keys()
                .filter(|path| path.as_str().starts_with(&under))
                .cloned()
                .collect();
            (dir, of)
        })
        .collect()
}

/// The files right under `crates/whyshelf-cli/tests/`.
fn cli_tests(files: &BTreeMap<RelPath, ArtifactKey>) -> Vec<(&RelPath, &ArtifactKey)> {
    files
        .iter()
        .filter(|(path, _)| {
            path.as_str()
                .strip_prefix(E2E)
                .is_some_and(|rest| !rest.contains('/'))
        })
        .collect()
}

/// Which helper directories a file right under `crates/whyshelf-cli/tests/` uses, from
/// its tokens, with a problem for a module it declares outside a `tests/<x>/mod.rs`.
fn uses(
    file: &RelPath,
    tokens: &[TokenTree],
    helpers: &BTreeMap<String, BTreeSet<RelPath>>,
) -> (BTreeSet<String>, Vec<String>) {
    let mut used = BTreeSet::new();
    let mut problems = Vec::new();
    for decl in mods(tokens) {
        if decl.path || !helpers.contains_key(&decl.name) {
            problems.push(format!(
                "{file}:{} declares mod {} in an integration test outside a tests/<x>/ directory: the guard cannot tell its users; put it under tests/{}/mod.rs ({DEC})",
                decl.line, decl.name, decl.name
            ));
        } else {
            used.insert(decl.name);
        }
    }
    (used, problems)
}

/// The regions the `Keep`s are read from: the include of the cover, and the files of
/// the end-to-end tests, read only when a helper lies outside the include.
struct Keeps {
    include: Option<Region>,
    e2e: Option<Region>,
}

impl Keeps {
    /// The `Keep` of `file` when the include takes it, or none.
    fn included(&self, file: &RelPath) -> Option<Keep> {
        self.include
            .as_ref()
            .filter(|region| region.files.contains_key(file))
            .map(|region| keep_in(&region.tests, file))
    }

    /// The `Keep` of a helper of the end-to-end tests: from the include when it takes
    /// it, else from the region of the end-to-end tests, read the first time it is
    /// needed. A helper that region does not take fails the run: as every helper is a
    /// file of the crates under it, that is met only when the tree changed while it was
    /// read (the spaces answer otherwise of the same files within one run).
    async fn helper(&mut self, app: &App, file: &RelPath) -> Result<Keep, Vec<String>> {
        if let Some(keep) = self.included(file) {
            return Ok(keep);
        }
        if self.e2e.is_none() {
            let e2e = region(app, vec![E2E_RUST.to_owned()], |c| {
                format!("the files of the end-to-end tests cannot all be listed: {c}")
            })
            .await?;
            self.e2e = Some(e2e);
        }
        let e2e = self.e2e.as_ref().expect("read above");
        if !e2e.files.contains_key(file) {
            return Err(vec![format!(
                "{file} is a helper of the end-to-end tests that {E2E_RUST} does not take (DEC-56)"
            )]);
        }
        Ok(keep_in(&e2e.tests, file))
    }
}

/// The files a verification names: those of the names under `tests` and under the
/// link with the preset `test`.
fn named_files<'a>(
    named: &'a Named,
    placed: &'a BTreeMap<ArtifactKey, RelPath>,
) -> BTreeSet<&'a RelPath> {
    named
        .tests
        .iter()
        .chain(&named.cases)
        .filter_map(|key| placed.get(key))
        .collect()
}

/// The problems of this repository's books against DEC-112 and DEC-56, read at `root`.
async fn guard(root: &Path) -> Vec<String> {
    match run(root).await {
        Ok(problems) | Err(problems) => problems,
    }
}

async fn run(root: &Path) -> Result<Vec<String>, Vec<String>> {
    let project = Project::new(root, None, None, None)
        .map_err(|e| vec![format!("the project cannot be read: {e}")])?;
    let app = ShelfApp::new(project);
    let map = mapped(&app).await.map_err(|e| vec![e])?;
    let link = test_link(&map)?;
    let docs = links_of(&app, &link.name).await?;
    let files = rust_files(&app).await?;
    let mut keeps = Keeps {
        include: include_region(&app, &link).await?,
        e2e: None,
    };
    let keys: BTreeSet<ArtifactKey> = docs
        .values()
        .flat_map(|named| named.tests.iter().chain(&named.cases))
        .cloned()
        .collect();
    let (placed, mut problems) = placed(&app, &keys).await?;
    let name = &link.name;

    // The names under the link with the preset test, and the files named whole, over
    // the files the include of its cover takes (DEC-112).
    for (key, named) in &docs {
        for test in &named.cases {
            let Some(file) = placed.get(test) else {
                continue;
            };
            let n = test.name();
            let Some(keep) = keeps.included(file) else {
                problems.push(format!(
                    "{key}: {n} under {name} is a test of {file}, which the include of its cover does not take: name {file} whole under tests, or add its region to the include in the same change ({DEC})"
                ));
                continue;
            };
            match keep {
                Keep::Part(why) => problems.push(format!(
                    "{key}: {n} under {name} is a test of {file}, whose tests cannot all be listed ({why}): name {file} whole under tests ({DEC})"
                )),
                Keep::CannotTell(c) => problems.push(format!(
                    "{key}: {n} under {name} is a test of {file}, of which rust-test cannot tell the tests ({c}) ({DEC})"
                )),
                Keep::Split(names) if names.contains(n) => {}
                Keep::Split(_) | Keep::NoTests => problems.push(format!(
                    "{key}: {file} holds no test {n} (check says absent or gap; also when spelled otherwise than listed) ({DEC})"
                )),
            }
        }
        for whole in &named.tests {
            let Some(file) = placed.get(whole) else {
                continue;
            };
            let Some(keep) = keeps.included(file) else {
                continue;
            };
            match keep {
                Keep::Split(_) => problems.push(format!(
                    "{key}: {file} is taken by the include of {name}: name its tests by function under {name}, not the file under tests ({DEC})"
                )),
                Keep::CannotTell(c) => problems.push(format!(
                    "{key}: {file} is named whole under tests, and rust-test cannot tell its tests ({c}) ({DEC})"
                )),
                Keep::NoTests | Keep::Part(_) => {}
            }
        }
    }

    // The helpers of the end-to-end tests (DEC-56).
    let helpers = cli_helpers(&files);
    let mut used: BTreeMap<&RelPath, BTreeSet<String>> = BTreeMap::new();
    let mut declared: BTreeSet<String> = BTreeSet::new();
    for (file, key) in cli_tests(&files) {
        let tokens = match text_of(&app, key, file)
            .await
            .and_then(|text| tokens_of(file, &text))
        {
            Ok(tokens) => tokens,
            Err(why) => {
                problems.push(why);
                continue;
            }
        };
        let (dirs, wrong) = uses(file, &tokens, &helpers);
        problems.extend(wrong);
        declared.extend(dirs.iter().cloned());
        used.insert(file, dirs);
    }
    for (dir, of) in &helpers {
        for file in of {
            let key = &files[file];
            if let Err(why) = text_of(&app, key, file)
                .await
                .and_then(|text| tokens_of(file, &text))
            {
                problems.push(why);
            }
            match keeps.helper(&app, file).await? {
                Keep::NoTests => {}
                Keep::CannotTell(c) => problems.push(format!(
                    "{file} is under the helpers {E2E}{dir}/, and rust-test cannot tell whether it holds a test ({c}) (DEC-56)"
                )),
                Keep::Part(_) | Keep::Split(_) if declared.contains(dir) => problems.push(format!(
                    "{file} is a helper of the end-to-end tests and holds a test or something that may make one: move it to a feature's file or drop it (DEC-56)"
                )),
                Keep::Part(_) | Keep::Split(_) => problems.push(format!(
                    "{file} holds a test, or something that may make one, and no file declares `mod {dir};`, so it is not compiled: move the test to a feature's file or remove it (DEC-56)"
                )),
            }
        }
    }
    for (key, named) in &docs {
        let whole: BTreeSet<&RelPath> = named.tests.iter().filter_map(|k| placed.get(k)).collect();
        for file in named_files(named, &placed) {
            for dir in used.get(file).into_iter().flatten() {
                for helper in &helpers[dir] {
                    if !whole.contains(helper) {
                        problems.push(format!(
                            "{key} names {file}, which uses the helpers {E2E}{dir}/, and does not name {helper} under tests (DEC-56)"
                        ));
                    }
                }
            }
        }
    }

    // No file of the end-to-end tests but the helpers is named by more than two
    // (DEC-56).
    let shared: BTreeSet<&RelPath> = helpers.values().flatten().collect();
    let mut namers: BTreeMap<&RelPath, Vec<&str>> = BTreeMap::new();
    for (key, named) in &docs {
        for file in named_files(named, &placed) {
            if file.as_str().starts_with(E2E) && !shared.contains(file) {
                namers.entry(file).or_default().push(key);
            }
        }
    }
    for (file, mut keys) in namers {
        if keys.len() > MOST {
            // V-9 before V-10: shorter keys first, then as written.
            keys.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
            problems.push(format!(
                "{file} is named, whole or by its tests, by {} verifications, more than {MOST}: {}; split it by feature (DEC-56)",
                keys.len(),
                keys.join(", ")
            ));
        }
    }
    Ok(problems)
}

/// The root of this repository.
fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[tokio::test]
async fn this_repository_names_its_tests_as_its_decisions_say() {
    let problems = guard(&repository()).await;
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// For each verification, what it names under `tests`, the files of the include it
/// names whole with the tests rust-test lists in each and the verifications that name
/// it, and the helpers of the end-to-end tests it should name: one line
/// `CANDIDATE <JSON>` each, for a batch of the move to be planned from. It fails
/// nothing.
#[tokio::test]
#[ignore = "prints the candidates of a batch; run it by hand"]
async fn candidates() {
    let app = ShelfApp::new(Project::new(repository(), None, None, None).expect("the project"));
    let map = mapped(&app).await.expect("the declaration");
    let link = test_link(&map).unwrap_or_else(|p| panic!("{}", p.join("\n")));
    let docs = links_of(&app, &link.name)
        .await
        .unwrap_or_else(|p| panic!("{}", p.join("\n")));
    let files = rust_files(&app)
        .await
        .unwrap_or_else(|p| panic!("{}", p.join("\n")));
    let keeps = Keeps {
        include: include_region(&app, &link)
            .await
            .unwrap_or_else(|p| panic!("{}", p.join("\n"))),
        e2e: None,
    };
    let keys: BTreeSet<ArtifactKey> = docs
        .values()
        .flat_map(|named| named.tests.iter().chain(&named.cases))
        .cloned()
        .collect();
    let (placed, _) = placed(&app, &keys).await.expect("the places");
    let helpers = cli_helpers(&files);
    let mut used: BTreeMap<&RelPath, BTreeSet<String>> = BTreeMap::new();
    for (file, key) in cli_tests(&files) {
        if let Ok(tokens) = text_of(&app, key, file)
            .await
            .and_then(|text| tokens_of(file, &text))
        {
            used.insert(file, uses(file, &tokens, &helpers).0);
        }
    }
    for (key, named) in &docs {
        let whole: Vec<&RelPath> = named.tests.iter().filter_map(|k| placed.get(k)).collect();
        let mut region = Vec::new();
        for (file, keep) in whole
            .iter()
            .filter_map(|file| keeps.included(file).map(|keep| (file, keep)))
        {
            let names = match keep {
                Keep::Split(names) => names.iter().map(|n| n.as_str().to_owned()).collect(),
                _ => Vec::new(),
            };
            let by: Vec<&str> = docs
                .iter()
                .filter(|(_, other)| named_files(other, &placed).contains(*file))
                .map(|(k, _)| k.as_str())
                .collect();
            region.push(serde_json::json!({ "file": file.as_str(), "names": names, "namers": by }));
        }
        let missing: Vec<&str> = named_files(named, &placed)
            .into_iter()
            .flat_map(|file| used.get(file).into_iter().flatten())
            .flat_map(|dir| &helpers[dir])
            .filter(|helper| !whole.contains(helper))
            .map(RelPath::as_str)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let line = serde_json::json!({
            "verification": key,
            "tests": whole.iter().map(|f| f.as_str()).collect::<Vec<_>>(),
            "region": region,
            "helpers": missing,
        });
        println!("CANDIDATE {line}");
    }
}

// The rule tests: each writes a temporary project of its own and reads it as the
// guard reads this repository.

/// How many directories this file's tests have made, to keep two apart.
static MADE: AtomicUsize = AtomicUsize::new(0);

/// A temporary directory, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let count = MADE.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "whyshelf-verification-spread-{test}-{}-{count}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// A Git repository here, run with the person's own settings shut out.
    fn git_init(&self) {
        let home = self.0.join(".home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("gitconfig"), "").unwrap();
        let mut command = Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(&name);
            }
        }
        let status = command
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .arg("-C")
            .arg(&self.0)
            .args(["init", "-q"])
            .status()
            .expect("git runs");
        assert!(status.success(), "git init");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A declaration of one book `verification` with `tests` (files) and `cases` (the
/// preset `test`), and `cover` written on `cases` when given.
fn declaration(cover: Option<&str>) -> String {
    let cover = cover.map_or_else(String::new, |c| format!("cover = {c}\n"));
    format!(
        "[defaults]\nartifact = \"file\"\n\n[[book]]\nname = \"verification\"\ndir = \"books/verification\"\nkey = {{ prefix = \"V\" }}\n[[book.field]]\nname = \"title\"\ntype = \"line\"\nrequired = true\n[[book.link]]\nname = \"tests\"\ntarget = \"artifact\"\nmeans = \"depends\"\ncompare = \"lines\"\n[[book.link]]\nname = \"cases\"\ntarget = \"artifact\"\npreset = \"test\"\n{cover}"
    )
}

/// A verification `V-<n>` that names `tests` and `cases`, each a JSON list.
fn verification(n: u32, tests: &str, cases: &str) -> String {
    format!(
        "---\nid: \"01a1c877-0000-7000-8000-{n:012}\"\nkey: \"V-{n}\"\ntitle: \"t\"\ntests: {tests}\ncases: {cases}\n---\nIt holds.\n"
    )
}

/// A project in a Git repository with this cover on `cases`, these verifications
/// (`(tests, cases)`, numbered from 1) and these files.
fn project(
    test: &str,
    cover: Option<&str>,
    docs: &[(&str, &str)],
    files: &[(&str, &str)],
) -> Scratch {
    let scratch = Scratch::new(test);
    scratch.write(".whyshelf/books.toml", declaration(cover));
    for (n, (tests, cases)) in (1..).zip(docs) {
        scratch.write(
            &format!("books/verification/V-{n}.md"),
            verification(n, tests, cases),
        );
    }
    for (path, text) in files {
        scratch.write(path, text);
    }
    scratch.git_init();
    scratch
}

fn assert_has(problems: &[String], needle: &str) {
    assert!(
        problems.iter().any(|p| p.contains(needle)),
        "no problem says {needle:?}: {problems:#?}"
    );
}

const ONE: &str = "#[test]\nfn one() {}\n";
const MAKER: &str = "#[derive(serde::Serialize)]\nstruct S;\n#[test]\nfn one() {}\n";
const COVER: &str = "{ include = [\"crates/x/src/**\"], parts = { whole = \"tests\" } }";

/// The guard's answer of the link with the preset test, read from a declaration
/// alone (no Git).
async fn link_of(books: Option<&str>) -> Result<TestLink, Vec<String>> {
    let scratch = Scratch::new("link");
    if let Some(books) = books {
        scratch.write(".whyshelf/books.toml", books);
    }
    let app = ShelfApp::new(Project::new(&scratch.0, None, None, None).unwrap());
    let map = mapped(&app).await.map_err(|e| vec![e])?;
    test_link(&map)
}

async fn link_problems(books: Option<&str>) -> Vec<String> {
    link_of(books).await.err().unwrap_or_default()
}

#[tokio::test]
async fn the_guard_reads_its_own_file_as_any_other_and_finds_no_helper_used() {
    let app = ShelfApp::new(Project::new(repository(), None, None, None).unwrap());
    let path: RelPath = "crates/whyshelf-cli/tests/verification_spread.rs"
        .parse()
        .unwrap();
    let key = ArtifactKey::new(space(FILE_SPACE), ArtifactName::new(path.as_str()).unwrap());
    let text = text_of(&app, &key, &path).await.unwrap();
    let tokens = tokens_of(&path, &text).unwrap();
    assert_eq!(mods(&tokens), []);
}

#[test]
fn a_module_is_read_from_the_tokens_at_the_top_and_from_nothing_in_a_literal_or_a_comment() {
    let path: RelPath = "crates/whyshelf-cli/tests/a.rs".parse().unwrap();
    let names = |text: &str| -> Vec<(String, bool)> {
        mods(&tokens_of(&path, text).unwrap())
            .into_iter()
            .map(|d| (d.name, d.path))
            .collect()
    };
    assert_eq!(names("mod common;\n"), [("common".to_owned(), false)]);
    assert_eq!(names("pub mod common;"), [("common".to_owned(), false)]);
    assert_eq!(
        names("pub(crate) mod common;"),
        [("common".to_owned(), false)]
    );
    assert_eq!(
        names("mod common; // helpers\n"),
        [("common".to_owned(), false)]
    );
    assert_eq!(
        names("#[path = \"x.rs\"]\nmod util;"),
        [("util".to_owned(), true)]
    );
    assert_eq!(
        names("mod common_more;"),
        [("common_more".to_owned(), false)]
    );
    for none in [
        "// mod common;\n",
        "/* mod common; */",
        "/// mod common;\nfn f() {}",
        "const T: &str = r#\"\nmod common;\n\"#;",
        "mod inline { mod common; }",
    ] {
        assert_eq!(names(none), [], "{none}");
    }
    assert!(tokens_of(&path, "fn f() { \"").is_err());
    assert_eq!(
        mods(&tokens_of(&path, "\n\nmod common;").unwrap())[0].line,
        3
    );
}

#[tokio::test]
async fn the_declaration_holds_one_link_with_the_preset_test_of_rust_test_and_its_cover_in_one_form()
 {
    let with = |extra: &str| format!("{}{extra}", declaration(None));
    // Read as a check reads it.
    assert_has(&link_problems(None).await, "the declaration cannot be read");
    for bad in ["true", "{ strength = \"sharp\", include = [\"a\"] }"] {
        assert_has(
            &link_problems(Some(&declaration(Some(bad)))).await,
            "the declaration has problems",
        );
    }
    assert_has(
        &link_problems(Some(&declaration(Some("{ include = [\"[\"] }")))).await,
        "the declaration has problems",
    );
    // One link with the preset test.
    let none = declaration(None).replace(
        "preset = \"test\"",
        "space = \"rust-test\"\nmeans = \"depends\"",
    );
    assert_has(
        &link_problems(Some(&none)).await,
        "has no link with the preset test",
    );
    let two =
        with("[[book.link]]\nname = \"test-fns\"\ntarget = \"artifact\"\npreset = \"test\"\n");
    assert_has(
        &link_problems(Some(&two)).await,
        "2 links with the preset test, cases, test-fns",
    );
    let renamed = declaration(None).replace("name = \"cases\"", "name = \"test-fns\"");
    assert_eq!(
        link_of(Some(&renamed))
            .await
            .ok()
            .map(|l| l.name.to_string()),
        Some("test-fns".to_owned())
    );
    // Of rust-test, whatever sets the space.
    let project_preset = format!(
        "[[preset]]\nname = \"test\"\nspace = \"file\"\nmeans = \"depends\"\n\n{}",
        declaration(None)
    );
    assert_has(
        &link_problems(Some(&project_preset)).await,
        "is of the space file, not rust-test",
    );
    let own_space =
        declaration(None).replace("preset = \"test\"", "preset = \"test\"\nspace = \"file\"");
    assert_has(
        &link_problems(Some(&own_space)).await,
        "is of the space file, not rust-test",
    );
    // The cover.
    assert!(link_of(Some(&declaration(None))).await.is_ok());
    assert!(link_of(Some(&declaration(Some(COVER)))).await.is_ok());
    let warn =
        "{ strength = \"warn\", include = [\"crates/x/src/**\"], parts = { whole = \"tests\" } }";
    assert_has(
        &link_problems(Some(&declaration(Some(warn)))).await,
        "declares a strength that is not error",
    );
    assert_has(
        &link_problems(Some(&declaration(Some(
            "{ include = [\"crates/x/src/**\"] }",
        ))))
        .await,
        "does not take its parts whole through tests",
    );
    let through_cases = "{ include = [\"crates/x/src/**\"], parts = { whole = \"cases\" } }";
    let problems = link_problems(Some(&declaration(Some(through_cases)))).await;
    assert!(
        problems.iter().any(
            |p| p.contains("does not take its parts whole through tests")
                || p.contains("the declaration has problems")
        ),
        "{problems:#?}"
    );
    let exclude = "{ include = [\"crates/x/src/**\"], exclude = [\"crates/x/src/a.rs\"], parts = { whole = \"tests\" } }";
    assert_has(
        &link_problems(Some(&declaration(Some(exclude)))).await,
        "writes an exclude",
    );
}

#[test]
fn a_name_lives_in_one_file_or_the_guard_cannot_read_it() {
    let key = ArtifactKey::new(
        space(RUST_TEST_SPACE),
        ArtifactName::new("crates/x/src/a.rs::one").unwrap(),
    );
    let files = |paths: &[&str]| -> whyshelf_core::domain::nonempty::NonEmptySet<RelPath> {
        whyshelf_core::domain::nonempty::NonEmptySet::from_set(
            paths.iter().map(|p| p.parse().unwrap()).collect(),
        )
        .unwrap()
    };
    assert_eq!(
        file_of(&key, &Placement::Only(files(&["crates/x/src/a.rs"]))),
        Ok("crates/x/src/a.rs".parse().unwrap())
    );
    for placement in [
        Placement::Only(files(&["crates/x/src", "crates/y/a.rs"])),
        Placement::Partly(files(&["crates/x/src/a.rs"])),
    ] {
        assert!(
            file_of(&key, &placement)
                .unwrap_err()
                .contains("is reached through no link")
        );
    }
    let why = whyshelf_core::domain::nonempty::NonBlank::new("the adapter's reason").unwrap();
    assert_eq!(
        file_of(&key, &Placement::Unplaced(why)),
        Err(format!(
            "rust-test places crates/x/src/a.rs::one nowhere: the adapter's reason ({DEC})"
        ))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_reached_through_a_link_or_placed_nowhere_is_refused_with_the_adapters_reason() {
    let linked = project(
        "linked",
        None,
        &[("[]", "[\"crates/l/a.rs::one\"]")],
        &[("crates/x/src/a.rs", ONE)],
    );
    std::os::unix::fs::symlink(linked.0.join("crates/x/src"), linked.0.join("crates/l")).unwrap();
    assert_has(&guard(&linked.0).await, "is reached through no link");

    let outside = project("outside", None, &[("[]", "[\"crates/o/a.rs::one\"]")], &[]);
    fs::create_dir_all(outside.0.join("crates")).unwrap();
    std::os::unix::fs::symlink(std::env::temp_dir(), outside.0.join("crates/o")).unwrap();
    assert_has(
        &guard(&outside.0).await,
        "rust-test places crates/o/a.rs::one nowhere: ",
    );

    let broken = project("broken", None, &[("[]", "[\"crates/b/a.rs::one\"]")], &[]);
    fs::create_dir_all(broken.0.join("crates")).unwrap();
    std::os::unix::fs::symlink(broken.0.join("gone"), broken.0.join("crates/b")).unwrap();
    assert_has(
        &guard(&broken.0).await,
        "rust-test places crates/b/a.rs::one nowhere: ",
    );

    // A text that is no name of rust-test is refused where a document's links are
    // read: the document is no valid one.
    let no_name = project("no-name", None, &[("[]", "[\"no test name\"]")], &[]);
    assert_has(&guard(&no_name.0).await, "V-1.md is no valid document");
}

#[tokio::test]
async fn a_verification_is_read_as_a_check_reads_it_and_one_that_is_not_valid_fails() {
    let text = project("text", None, &[("\"crates/x/a.rs\"", "[]")], &[]);
    assert_has(
        &guard(&text.0).await,
        "books/verification/V-1.md is no valid document",
    );
    let blank = project("blank", None, &[("[\"crates/x/a.rs\", \"\"]", "[]")], &[]);
    assert_has(
        &guard(&blank.0).await,
        "books/verification/V-1.md is no valid document",
    );
    let nothing = project(
        "nothing",
        None,
        &[("null", "null"), ("[]", "[]")],
        &[("crates/x/src/a.rs", ONE)],
    );
    assert_eq!(guard(&nothing.0).await, Vec::<String>::new());
}

#[tokio::test]
async fn a_file_of_the_end_to_end_tests_the_state_cannot_give_or_that_is_not_utf8_fails() {
    // Past the bound of a file, it cannot be told: the files of the crates are not
    // all listed, before any is read.
    let big = project("big", None, &[], &[]);
    big.write(
        "crates/whyshelf-cli/tests/big.rs",
        vec![b' '; 16 * 1024 * 1024 + 1],
    );
    assert_has(
        &guard(&big.0).await,
        "the files of the crates cannot all be listed",
    );
    let latin = project("latin", None, &[], &[]);
    latin.write("crates/whyshelf-cli/tests/latin.rs", b"// \xff\n");
    assert_has(&guard(&latin.0).await, "latin.rs is not UTF-8");
    let unlexed = project(
        "unlexed",
        None,
        &[],
        &[
            ("crates/whyshelf-cli/tests/a.rs", "fn f() { \""),
            ("crates/whyshelf-cli/tests/b.rs", "fn g() { \""),
        ],
    );
    let problems = guard(&unlexed.0).await;
    let lex: Vec<&String> = problems
        .iter()
        .filter(|p| p.contains("cannot be read as Rust tokens"))
        .collect();
    assert_eq!(lex.len(), 2, "{problems:#?}");
}

#[tokio::test]
async fn the_files_of_the_crates_outside_git_cannot_all_be_listed() {
    let scratch = Scratch::new("no-git");
    scratch.write(".whyshelf/books.toml", declaration(None));
    scratch.write("crates/x/src/a.rs", ONE);
    assert_has(
        &guard(&scratch.0).await,
        "the files of the crates cannot all be listed",
    );
}

#[tokio::test]
async fn a_name_under_cases_is_a_test_rust_test_lists_of_a_file_the_include_takes() {
    let files = [
        ("crates/x/src/a.rs", ONE),
        ("crates/x/src/m.rs", MAKER),
        ("crates/y/b.rs", ONE),
    ];
    let named = project(
        "named",
        Some(COVER),
        &[(
            "[\"crates/x/src/m.rs\", \"crates/y/b.rs\"]",
            "[\"crates/x/src/a.rs::one\"]",
        )],
        &files,
    );
    assert_eq!(guard(&named.0).await, Vec::<String>::new());

    let wrong = project(
        "wrong",
        Some(COVER),
        &[(
            "[]",
            "[\"crates/y/b.rs::one\", \"crates/x/src/m.rs::one\", \"crates/x/src/a.rs::two\", \"crates/x/src/a.rs::r#one\"]",
        )],
        &files,
    );
    let problems = guard(&wrong.0).await;
    assert_has(
        &problems,
        "crates/y/b.rs::one under cases is a test of crates/y/b.rs, which the include of its cover does not take",
    );
    assert_has(
        &problems,
        "crates/x/src/m.rs::one under cases is a test of crates/x/src/m.rs, whose tests cannot all be listed",
    );
    assert_has(
        &problems,
        "crates/x/src/a.rs holds no test crates/x/src/a.rs::two",
    );
    assert_has(
        &problems,
        "crates/x/src/a.rs holds no test crates/x/src/a.rs::r#one",
    );

    // With no cover, the include takes nothing: every name under cases fails, every
    // file whole passes.
    let uncovered = project(
        "uncovered",
        None,
        &[("[\"crates/x/src/a.rs\"]", "[\"crates/x/src/a.rs::one\"]")],
        &files,
    );
    let problems = guard(&uncovered.0).await;
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert_has(&problems, "which the include of its cover does not take");
}

#[tokio::test]
async fn a_file_the_include_takes_is_named_whole_only_when_its_tests_are_not_listed_one_by_one() {
    let files = [
        ("crates/x/src/a.rs", ONE),
        ("crates/x/src/m.rs", MAKER),
        ("crates/x/src/none.rs", "pub fn f() {}\n"),
        ("crates/x/src/data.toml", "x = 1\n"),
        ("crates/y/b.rs", ONE),
    ];
    let whole = project(
        "whole",
        Some(COVER),
        &[(
            "[\"crates/x/src/a.rs\", \"crates/x/src/m.rs\", \"crates/x/src/none.rs\", \"crates/x/src/data.toml\", \"crates/y/b.rs\"]",
            "[]",
        )],
        &files,
    );
    let problems = guard(&whole.0).await;
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert_has(
        &problems,
        "crates/x/src/a.rs is taken by the include of cases: name its tests by function under cases",
    );

    let scratch = Scratch::new("include-no-git");
    scratch.write(".whyshelf/books.toml", declaration(Some(COVER)));
    let problems = guard(&scratch.0).await;
    assert!(
        problems.iter().any(|p| p.contains("cannot all be listed")),
        "{problems:#?}"
    );
}

#[tokio::test]
async fn a_file_whose_path_is_a_pattern_is_read_from_the_one_listing_as_any_other() {
    let cover = "{ include = [\"crates/x/**\"], parts = { whole = \"tests\" } }";
    let files = [("crates/x/a.rs", ONE), ("crates/x/*.rs", ONE)];
    let by_function = project(
        "pattern",
        Some(cover),
        &[("[]", "[\"crates/x/a.rs::one\", \"crates/x/*.rs::one\"]")],
        &files,
    );
    assert_eq!(guard(&by_function.0).await, Vec::<String>::new());

    let whole = project(
        "pattern-whole",
        Some(cover),
        &[("[\"crates/x/*.rs\"]", "[]")],
        &files,
    );
    let problems = guard(&whole.0).await;
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert_has(
        &problems,
        "crates/x/*.rs is taken by the include of cases: name its tests by function under cases",
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_file_the_include_takes_reached_through_a_link_is_refused() {
    let cover = "{ include = [\"x/src/**\"], parts = { whole = \"tests\" } }";
    let refused = "file places x/src/b.rs in x/src/a.rs, x/src/b.rs: a test file of this repository is reached through no link";
    for (test, cases) in [
        ("include-link", "[]"),
        ("include-link-named", "[\"x/src/a.rs::one\"]"),
    ] {
        let linked = project(test, Some(cover), &[("[]", cases)], &[("x/src/a.rs", ONE)]);
        std::os::unix::fs::symlink(linked.0.join("x/src/a.rs"), linked.0.join("x/src/b.rs"))
            .unwrap();
        let problems = guard(&linked.0).await;
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert_has(&problems, refused);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_rust_file_of_the_crates_reached_through_a_link_is_refused_before_any_region_is_read() {
    let linked = project(
        "crates-link",
        Some(COVER),
        &[],
        &[("crates/x/src/a.rs", ONE)],
    );
    std::os::unix::fs::symlink(
        linked.0.join("crates/x/src/a.rs"),
        linked.0.join("crates/x/src/b.rs"),
    )
    .unwrap();
    let problems = guard(&linked.0).await;
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert_has(
        &problems,
        "file places crates/x/src/b.rs in crates/x/src/a.rs, crates/x/src/b.rs: a test file of this repository is reached through no link",
    );
}

#[test]
fn a_listing_says_no_test_a_part_tests_one_by_one_or_that_it_cannot_tell() {
    use whyshelf_core::domain::nonempty::{NonBlank, NonEmptyVec};
    use whyshelf_core::domain::shelf::artifact::{NameListing, PartRead};
    let rust_test = space(RUST_TEST_SPACE);
    let selection = selection_of(vec!["*.rs".to_owned()]).unwrap();
    let path = |text: &str| -> RelPath { text.parse().unwrap() };
    let file_key =
        |text: &str| ArtifactKey::new(space(FILE_SPACE), ArtifactName::new(text).unwrap());
    let (a, b) = (path("a.rs"), path("b.rs"));
    // The one listing takes both files.
    let files = BTreeMap::from([(a.clone(), file_key("a.rs")), (b.clone(), file_key("b.rs"))]);
    let name = ArtifactName::new("a.rs::one").unwrap();
    let at_a = |names: &BTreeSet<ArtifactName>| -> BTreeMap<ArtifactKey, RelPath> {
        names
            .iter()
            .map(|n| (ArtifactKey::new(rust_test.clone(), n.clone()), a.clone()))
            .collect()
    };
    // Read as the domain reads a listing, with its parts, each name placed in a.rs.
    let read = |answer: NameListing| -> Result<Tests, Completeness> {
        let listings = Listings::Asked(BTreeMap::from([(
            (rust_test.clone(), selection.clone()),
            answer,
        )]));
        ListedNames::of(&listings)
            .names_and_parts(&rust_test, &selection)
            .map(|(names, parts)| {
                let (tests, problems) = grouped(&at_a(names), parts, &files);
                assert_eq!(problems, Vec::<String>::new());
                tests
            })
    };
    let why = |text: &str| NonBlank::new(text).unwrap();

    let none = read(NameListing::All(BTreeSet::new()));
    assert_eq!(keep_in(&none, &a), Keep::NoTests);
    assert_eq!(keep_in(&none, &b), Keep::NoTests);

    let one = read(NameListing::All(BTreeSet::from([name.clone()])));
    assert_eq!(
        keep_in(&one, &a),
        Keep::Split(BTreeSet::from([name.clone()]))
    );
    assert_eq!(keep_in(&one, &b), Keep::NoTests);

    // The part of b makes b a part, and not a.
    let part = |of: &str| {
        ListingPart::new(
            file_key(of),
            why("a macro may make a test"),
            PartRead::Found(BTreeSet::new()),
        )
    };
    let parted = read(NameListing::Parted {
        found: BTreeSet::from([name.clone()]),
        parts: NonEmptyVec::new(part("b.rs"), Vec::new()),
    });
    assert_eq!(
        keep_in(&parted, &a),
        Keep::Split(BTreeSet::from([name.clone()]))
    );
    assert_eq!(
        keep_in(&parted, &b),
        Keep::Part("a macro may make a test".to_owned())
    );

    // A listing that could not be made, or was cut at a bound, tells nothing of any
    // file of its region: it is neither a part nor a file's tests.
    let cannot = read(NameListing::CannotList(why("outside Git")));
    let cut = read(NameListing::Some {
        found: BTreeSet::from([name.clone()]),
        why: why("past the bound of names"),
    });
    for file in [&a, &b] {
        assert!(matches!(
            keep_in(&cannot, file),
            Keep::CannotTell(Completeness::CannotList(_))
        ));
        assert!(matches!(
            keep_in(&cut, file),
            Keep::CannotTell(Completeness::Partial(_))
        ));
    }

    // A name or a part of no file of the region is said, not read as of another.
    let elsewhere = BTreeMap::from([(
        ArtifactKey::new(rust_test.clone(), ArtifactName::new("c.rs::one").unwrap()),
        path("c.rs"),
    )]);
    let (tests, problems) = grouped(&elsewhere, &[part("c.rs")], &files);
    assert!(tests.names.is_empty() && tests.parts.is_empty());
    assert_eq!(problems.len(), 2, "{problems:#?}");
}

#[tokio::test]
async fn a_verification_over_an_end_to_end_file_names_the_helpers_it_uses() {
    let helper = (
        "crates/whyshelf-cli/tests/common/mod.rs",
        "pub fn help() {}\n",
    );
    let using = project(
        "helpers",
        None,
        &[
            ("[\"crates/whyshelf-cli/tests/a.rs\"]", "[]"),
            (
                "[\"crates/whyshelf-cli/tests/b.rs\", \"crates/whyshelf-cli/tests/common/mod.rs\"]",
                "[]",
            ),
            ("[\"crates/whyshelf-cli/tests/c.rs\"]", "[]"),
        ],
        &[
            (
                "crates/whyshelf-cli/tests/a.rs",
                "mod common;\n#[test]\nfn a() {}\n",
            ),
            (
                "crates/whyshelf-cli/tests/b.rs",
                "pub mod common;\n#[test]\nfn b() {}\n",
            ),
            (
                "crates/whyshelf-cli/tests/c.rs",
                "// mod common;\n#[test]\nfn c() {}\n",
            ),
            helper,
        ],
    );
    let problems = guard(&using.0).await;
    assert_eq!(
        problems,
        [
            "V-1 names crates/whyshelf-cli/tests/a.rs, which uses the helpers crates/whyshelf-cli/tests/common/, and does not name crates/whyshelf-cli/tests/common/mod.rs under tests (DEC-56)"
        ]
    );

    for (holds, text) in [
        ("test", "#[test]\nfn t() {}\n"),
        ("derive", "#[derive(serde::Serialize)]\npub struct S;\n"),
    ] {
        let helped = project(
            holds,
            None,
            &[],
            &[
                ("crates/whyshelf-cli/tests/a.rs", "mod common;\n"),
                ("crates/whyshelf-cli/tests/common/mod.rs", text),
            ],
        );
        assert_has(
            &guard(&helped.0).await,
            "common/mod.rs is a helper of the end-to-end tests and holds a test or something that may make one",
        );
    }

    let undeclared = project(
        "undeclared",
        None,
        &[],
        &[(
            "crates/whyshelf-cli/tests/util/mod.rs",
            "#[test]\nfn t() {}\n",
        )],
    );
    assert_has(
        &guard(&undeclared.0).await,
        "and no file declares `mod util;`",
    );

    let outside = project(
        "outside-dir",
        None,
        &[],
        &[
            ("crates/whyshelf-cli/tests/a.rs", "mod util;\n"),
            ("crates/whyshelf-cli/tests/util.rs", "pub fn f() {}\n"),
            (
                "crates/whyshelf-cli/tests/b.rs",
                "#[path = \"x.rs\"]\nmod common;\n",
            ),
            helper,
        ],
    );
    let problems = guard(&outside.0).await;
    assert_has(
        &problems,
        "a.rs:1 declares mod util in an integration test outside a tests/<x>/ directory",
    );
    assert_has(
        &problems,
        "b.rs:2 declares mod common in an integration test outside a tests/<x>/ directory",
    );

    // A helper of another crate asks nothing.
    let other = project(
        "other-crate",
        None,
        &[("[\"crates/x/tests/a.rs\"]", "[]")],
        &[
            ("crates/x/tests/a.rs", "mod common;\n#[test]\nfn a() {}\n"),
            ("crates/x/tests/common/mod.rs", "pub fn help() {}\n"),
        ],
    );
    assert_eq!(guard(&other.0).await, Vec::<String>::new());
}

#[tokio::test]
async fn a_helper_the_include_takes_is_read_from_the_include_and_no_other_region() {
    let cover =
        "{ include = [\"crates/whyshelf-cli/tests/**/*.rs\"], parts = { whole = \"tests\" } }";
    let user = ("crates/whyshelf-cli/tests/a.rs", "mod common;\n");
    let helper: RelPath = "crates/whyshelf-cli/tests/common/mod.rs".parse().unwrap();
    for (test, text, refused) in [
        ("include-helper-test", "#[test]\nfn t() {}\n", true),
        ("include-helper-none", "pub fn help() {}\n", false),
    ] {
        let helped = project(test, Some(cover), &[], &[user, (helper.as_str(), text)]);
        let problems = guard(&helped.0).await;
        if refused {
            assert_eq!(problems.len(), 1, "{problems:#?}");
            assert_has(
                &problems,
                "common/mod.rs is a helper of the end-to-end tests and holds a test or something that may make one",
            );
        } else {
            assert_eq!(problems, Vec::<String>::new());
        }

        // The Keep is the include's: the region of the end-to-end tests is not read.
        let app = ShelfApp::new(Project::new(&helped.0, None, None, None).unwrap());
        let map = mapped(&app).await.unwrap();
        let link = test_link(&map).unwrap();
        let mut keeps = Keeps {
            include: include_region(&app, &link).await.unwrap(),
            e2e: None,
        };
        let keep = keeps.helper(&app, &helper).await.unwrap();
        assert_eq!(matches!(keep, Keep::Split(_)), refused, "{keep:?}");
        assert!(keeps.e2e.is_none());
    }
}

#[tokio::test]
async fn no_file_of_the_end_to_end_tests_is_named_by_more_than_two_verifications() {
    let file = ("crates/whyshelf-cli/tests/a.rs", "#[test]\nfn a() {}\n");
    let three = project(
        "three",
        None,
        &[
            ("[\"crates/whyshelf-cli/tests/a.rs\"]", "[]"),
            ("[\"crates/whyshelf-cli/tests/a.rs\"]", "[]"),
            ("[\"crates/whyshelf-cli/tests/a.rs\"]", "[]"),
        ],
        &[file],
    );
    assert_has(
        &guard(&three.0).await,
        "crates/whyshelf-cli/tests/a.rs is named, whole or by its tests, by 3 verifications, more than 2: V-1, V-2, V-3",
    );
    let two = project(
        "two",
        None,
        &[
            ("[\"crates/whyshelf-cli/tests/a.rs\"]", "[]"),
            ("[\"crates/whyshelf-cli/tests/a.rs\"]", "[]"),
        ],
        &[file],
    );
    assert_eq!(guard(&two.0).await, Vec::<String>::new());
    // By its tests, under the cover: counted by the file rust-test places them in.
    let cover = "{ include = [\"crates/whyshelf-cli/tests/**\"], parts = { whole = \"tests\" } }";
    let by_tests = project(
        "by-tests",
        Some(cover),
        &[
            ("[]", "[\"crates/whyshelf-cli/tests/a.rs::a\"]"),
            ("[]", "[\"crates/whyshelf-cli/tests/a.rs::a\"]"),
            ("[]", "[\"crates/whyshelf-cli/tests/a.rs::a\"]"),
        ],
        &[file],
    );
    assert_has(&guard(&by_tests.0).await, "by 3 verifications, more than 2");
}

#[test]
fn a_scratch_directory_is_removed_when_it_is_dropped() {
    let scratch = Scratch::new("drop");
    scratch.write("a/b.txt", "x");
    let dir = scratch.0.clone();
    assert!(dir.join("a/b.txt").exists());
    drop(scratch);
    assert!(!dir.exists());
}
