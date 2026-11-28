//! The docs of the tests of `catalog` and `moves`, written by hand in the layout
//! rustdoc 1.95 writes (its classes and the order of its elements), with words of this
//! repository only: no page rustdoc wrote, and no text of the standard library, is
//! copied into one.
//!
//! | what | what it is |
//! |---|---|
//! | [`write`] | a file of a tree of docs, its directories made |
//! | [`page`], [`index`], [`redirect`] | as much of a page as finding a name needs: the anchors of its members, the re-exports of a module, a page that only redirects |
//! | [`whole`], [`item_page`], [`module_index`] | a page laid out whole, so that what it promises is read |

use std::path::{Path, PathBuf};

use whyshelf_core::app::shelf::Side;

use super::docs_tree::DocsTree;
use super::held::HeldDocs;
use super::layout::GENERATOR_MARK;
use super::lookup::Lookup;

/// A lookup of the docs at `docs` of the project at `root`, as the project is, held
/// against nothing ([`HeldDocs::for_walk_tests`]): what a test of how a page is walked
/// makes, with no space around it.
pub(super) async fn walk_lookup(root: &Path, docs: PathBuf, max_page: u64) -> Lookup {
    Lookup::on(
        HeldDocs::for_walk_tests(DocsTree::at(root, docs).await, Side::After),
        max_page,
    )
}

/// The file `at` under `dir`, with `text`, its directories made. A test that cannot
/// write its docs stops (this file is no production code, but a page that is not
/// written is no test).
pub(super) async fn write(dir: &Path, at: &str, text: &str) {
    let full = dir.join(at);
    let stop = |e: std::io::Error| panic!("{} cannot be written: {e}", full.display());
    match full.parent() {
        Some(parent) => tokio::fs::create_dir_all(parent).await.unwrap_or_else(stop),
        None => stop(std::io::ErrorKind::NotFound.into()),
    }
    tokio::fs::write(&full, text).await.unwrap_or_else(stop);
}

/// A page of the source as rustdoc 1.95 lays one out, of the file of text `text`: the
/// code block of the file, each line after the anchor of its number.
pub(super) fn source_page(text: &str) -> String {
    let mut code = String::new();
    for (at, line) in text.split_inclusive('\n').enumerate() {
        let n = at + 1;
        let line = line
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        code.push_str(&format!(
            "<a href=#{n} id={n} data-nosnippet>{n}</a><span class=\"kw\">{line}</span>"
        ));
    }
    format!(
        "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><main><section id=\"main-content\" class=\"content\"><div class=\"main-heading\"><h1><div>Source</div></h1></div><div class=\"example-wrap digits-1\"><pre class=\"rust\"><code>{code}</code></pre></div></section></main></body></html>"
    )
}

/// The text of the root file of the crate `krate` that [`sources`] writes.
pub(super) fn lib_text(krate: &str) -> String {
    format!("//! The crate `{krate}` of a test.\n")
}

/// The sources of the docs at `docs`: a workspace at `root` with a package of each of
/// `crates` under `crates/<name>`, each a library `src/lib.rs` of [`lib_text`], and the
/// page of the source of each in the docs, so that the docs are of these sources.
pub(super) async fn sources(root: &Path, docs: &Path, crates: &[&str]) {
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .await;
    for krate in crates {
        write(
            root,
            &format!("crates/{krate}/Cargo.toml"),
            &format!("[package]\nname = \"{krate}\"\n"),
        )
        .await;
        write(
            root,
            &format!("crates/{krate}/src/lib.rs"),
            &lib_text(krate),
        )
        .await;
        write(
            docs,
            &format!("src/{krate}/lib.rs.html"),
            &source_page(&lib_text(krate)),
        )
        .await;
    }
}

/// A page with these anchors, as rustdoc writes them.
pub(super) fn page(ids: &[&str]) -> String {
    let anchors: String = ids
        .iter()
        .map(|id| format!("<section id=\"{id}\" class=\"method\"></section>"))
        .collect();
    format!(
        "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><main>{anchors}</main></body></html>"
    )
}

/// The index of a module with these re-exports, each `(name, link)`.
pub(super) fn index(reexports: &[(&str, Option<&str>)]) -> String {
    let entries: String = reexports
        .iter()
        .map(|(name, link)| match link {
            Some(href) => format!(
                "<dt id=\"reexport.{name}\"><code>pub use inner::<a class=\"struct\" href=\"{href}\" title=\"x\">{name}</a>;</code></dt>"
            ),
            None => format!("<dt id=\"reexport.{name}\"><code>pub use {name};</code></dt>"),
        })
        .collect();
    format!(
        "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><h2 id=\"reexports\">Re-exports</h2><dl>{entries}</dl></body></html>"
    )
}

/// A page rustdoc writes for an item of a private module: it only redirects.
pub(super) fn redirect(to: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta http-equiv=\"refresh\" content=\"0;URL={to}\"><title>Redirection</title></head><body><p>Redirecting to <a href=\"{to}\">{to}</a>...</p></body></html>"
    )
}

/// A page laid out whole, as rustdoc lays out that of an item: a sidebar with a
/// heading, then the main content.
pub(super) fn whole(main: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><nav class=\"sidebar\"><h2 class=\"location\">Sidebar</h2></nav><main><section id=\"main-content\" class=\"content\"><div class=\"main-heading\"><h1>Item</h1><span class=\"sub-heading\"><a class=\"src\" href=\"s.html#1\">Source</a> </span></div>{main}</section></main></body></html>"
    )
}

/// The top doc of an item, in one paragraph.
fn top_doc(text: &str) -> String {
    format!(
        "<details class=\"toggle top-doc\" open><summary class=\"hideme\"><span>Expand description</span></summary><div class=\"docblock\"><p>{text}</p>\n</div></details>"
    )
}

/// A member as the page of its type lays it out: `(name, signature)`, with no docs.
fn member(name: &str, signature: &str) -> String {
    format!(
        "<section id=\"method.{name}\" class=\"method\"><a class=\"src rightside\" href=\"s.html#1\">Source</a><h4 class=\"code-header\">{signature}</h4></section>"
    )
}

/// The page of a struct (or any item with a declaration): its declaration `decl`, its
/// docs, and these methods, each `(name, signature)`.
pub(super) fn item_page(decl: &str, docs: Option<&str>, methods: &[(&str, &str)]) -> String {
    let docs = docs.map(top_doc).unwrap_or_default();
    let methods: String = methods
        .iter()
        .map(|(name, signature)| member(name, signature))
        .collect();
    whole(&format!(
        "<pre class=\"rust item-decl\"><code>{decl}</code></pre>{docs}<div id=\"implementations-list\"><div class=\"impl-items\">{methods}</div></div>"
    ))
}

/// The index of a module (or a crate) with these docs, laid out whole.
pub(super) fn module_index(docs: &str) -> String {
    whole(&format!(
        "{}<h2 id=\"structs\" class=\"section-header\">Structs</h2>",
        top_doc(docs)
    ))
}
