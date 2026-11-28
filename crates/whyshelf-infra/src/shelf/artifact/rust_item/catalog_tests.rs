//! Tests of the catalog of the docs, kept apart from the module so that the module
//! reads the docs through the lookup alone, as its rule of architecture holds.

use std::path::Path;

use super::super::fixture::{index, page, redirect, write};
use super::*;

const PAGE_BOUND: u64 = 1024 * 1024;

fn limits() -> CatalogLimits {
    CatalogLimits {
        page_bytes: PAGE_BOUND,
        ..CatalogLimits::default()
    }
}

async fn lookup_at(target: &Path) -> Lookup {
    super::super::fixture::walk_lookup(target, target.join("doc"), PAGE_BOUND).await
}

fn path(text: &str) -> RustPath {
    RustPath::parse(text).unwrap()
}

fn texts(catalog: &Catalog) -> Vec<String> {
    catalog.names().keys().map(RustPath::as_name).collect()
}

/// Docs laid out as rustdoc writes them: the crate `demo` with a module `inner`,
/// a re-export, a private module whose pages only redirect, and the directories of
/// the docs that are no crate's; the crate `demo_infra`; the crate `fake`, whose
/// index is not rustdoc's.
async fn docs() -> tempfile::TempDir {
    let target = tempfile::tempdir().unwrap();
    let d = target.path();
    write(
        d,
        "doc/demo/index.html",
        &index(&[("Thing", Some("inner/struct.Thing.html"))]),
    )
    .await;
    write(d, "doc/demo/struct.Shelf.html", &page(&["method.build"])).await;
    write(d, "doc/demo/enum.Cause.html", &page(&["variant.Gone"])).await;
    write(d, "doc/demo/fn.expand.html", &page(&[])).await;
    // Two kinds of one name: the first of the order of kinds is the one read.
    write(d, "doc/demo/struct.Twice.html", &page(&[])).await;
    write(d, "doc/demo/fn.Twice.html", &page(&[])).await;
    write(d, "doc/demo/sidebar-items.js", "").await;
    write(d, "doc/demo/all.html", &page(&[])).await;
    write(d, "doc/demo/inner/index.html", &index(&[])).await;
    write(
        d,
        "doc/demo/inner/struct.Thing.html",
        &page(&["method.new"]),
    )
    .await;
    write(d, "doc/demo/inner/fn.helper.html", &page(&[])).await;
    write(d, "doc/demo/inner/deep/index.html", &index(&[])).await;
    write(d, "doc/demo/inner/deep/fn.deeper.html", &page(&[])).await;
    write(
        d,
        "doc/demo/hidden/fn.expand.html",
        &redirect("../../demo/fn.expand.html"),
    )
    .await;
    write(d, "doc/demo_infra/index.html", &index(&[])).await;
    write(d, "doc/demo_infra/fn.open.html", &page(&[])).await;
    write(
        d,
        "doc/fake/index.html",
        "<!DOCTYPE html><html><head></head><body></body></html>",
    )
    .await;
    write(d, "doc/fake/struct.A.html", &page(&[])).await;
    write(d, "doc/src/demo/lib.rs.html", &page(&[])).await;
    write(d, "doc/static.files/main.js", "").await;
    write(d, "doc/trait.impl/demo/trait.X.js", "").await;
    write(d, "doc/help.html", &page(&[])).await;
    target
}

#[tokio::test]
async fn the_catalog_is_the_modules_and_the_items_of_the_crates_rustdoc_wrote() {
    let target = docs().await;
    let mut lookup = lookup_at(target.path()).await;
    let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
    assert!(catalog.listed());
    assert!(!catalog.cut());
    assert_eq!(catalog.unreadable(), 0);
    // No re-export (`demo::Thing`), no redirect page (`demo::hidden`), no page that
    // is no item's, no crate whose index is not rustdoc's, no directory of the docs
    // that is no crate.
    assert_eq!(
        texts(&catalog),
        [
            "demo",
            "demo::Cause",
            "demo::Shelf",
            "demo::Twice",
            "demo::expand",
            "demo::inner",
            "demo::inner::Thing",
            "demo::inner::deep",
            "demo::inner::deep::deeper",
            "demo::inner::helper",
            "demo_infra",
            "demo_infra::open",
        ]
    );
    // The first kind in the order of kinds tells `Twice`.
    assert!(matches!(
        catalog.names().get(&path("demo::Twice")),
        Some(Place::Item { kind, .. }) if kind == "struct"
    ));
}

#[tokio::test]
async fn every_name_of_the_catalog_is_one_the_lookup_finds_there_at_its_place() {
    let target = docs().await;
    let mut lookup = lookup_at(target.path()).await;
    let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
    assert!(!catalog.names().is_empty());
    for (name, place) in catalog.names() {
        assert_eq!(
            lookup.find(name).await,
            Found::There(place.clone()),
            "{}",
            name.as_name()
        );
    }
}

#[tokio::test]
async fn a_module_is_told_before_an_item_of_its_name() {
    let target = docs().await;
    // `inner` is a module and a function of one name: `find` reads the module.
    write(target.path(), "doc/demo/fn.inner.html", &page(&[])).await;
    let mut lookup = lookup_at(target.path()).await;
    let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
    assert!(matches!(
        catalog.names().get(&path("demo::inner")),
        Some(Place::Module { .. })
    ));
    for (name, place) in catalog.names() {
        assert_eq!(lookup.find(name).await, Found::There(place.clone()));
    }
}

#[tokio::test]
async fn the_crates_of_the_gone_names_are_walked_first() {
    let target = docs().await;
    let mut lookup = lookup_at(target.path()).await;
    let bound = CatalogLimits {
        names: 2,
        ..limits()
    };
    let catalog = Catalog::walk(&mut lookup, &["demo_infra".to_owned()], &bound).await;
    assert!(catalog.cut());
    assert_eq!(texts(&catalog), ["demo_infra", "demo_infra::open"]);
    // A crate named that the docs do not have is no crate to walk.
    let mut lookup = lookup_at(target.path()).await;
    let catalog = Catalog::walk(&mut lookup, &["gone".to_owned()], &limits()).await;
    assert!(texts(&catalog).contains(&"demo::Shelf".to_owned()));
}

#[tokio::test]
async fn a_walk_past_the_most_names_stops_and_says_so() {
    let target = docs().await;
    let mut lookup = lookup_at(target.path()).await;
    let bound = CatalogLimits {
        names: 3,
        ..limits()
    };
    let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
    assert!(catalog.cut());
    assert_eq!(catalog.names().len(), 3);
    // Exactly the bound is not past it.
    let mut lookup = lookup_at(target.path()).await;
    let all = Catalog::walk(&mut lookup, &[], &limits())
        .await
        .names()
        .len();
    let mut lookup = lookup_at(target.path()).await;
    let bound = CatalogLimits {
        names: all,
        ..limits()
    };
    let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
    assert!(!catalog.cut());
    assert_eq!(catalog.names().len(), all);
}

#[tokio::test]
async fn a_module_nested_past_the_deepest_is_named_and_not_walked() {
    let target = docs().await;
    let mut lookup = lookup_at(target.path()).await;
    // The crate is at 0, `inner` at 1, `deep` at 2: `deep` is named, and what is
    // in it is not reached.
    let bound = CatalogLimits {
        depth: 1,
        ..limits()
    };
    let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
    assert!(catalog.cut());
    let names = texts(&catalog);
    assert!(names.contains(&"demo::inner::deep".to_owned()));
    assert!(!names.contains(&"demo::inner::deep::deeper".to_owned()));
    assert!(names.contains(&"demo::inner::helper".to_owned()));
}

#[tokio::test]
async fn a_walk_that_has_read_the_most_pages_or_bytes_reads_no_more_crates() {
    let target = docs().await;
    let mut lookup = lookup_at(target.path()).await;
    let bound = CatalogLimits {
        pages: 1,
        ..limits()
    };
    let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
    assert!(catalog.cut());
    // The first crate was read whole; the next was not read.
    assert!(texts(&catalog).iter().all(|name| name.starts_with("demo")));
    assert!(!texts(&catalog).contains(&"demo_infra".to_owned()));
    assert_eq!(lookup.reads().pages, 1);

    let mut lookup = lookup_at(target.path()).await;
    let bound = CatalogLimits {
        bytes: 1,
        ..limits()
    };
    let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
    assert!(catalog.cut());
    assert!(!texts(&catalog).contains(&"demo_infra".to_owned()));
}

#[tokio::test]
async fn docs_that_are_not_there_are_no_catalog_and_say_so() {
    let empty = tempfile::tempdir().unwrap();
    let mut lookup = lookup_at(empty.path()).await;
    let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
    assert!(!catalog.listed());
    assert!(catalog.names().is_empty());
    assert!(!catalog.cut());
}

#[tokio::test]
async fn a_crate_with_a_page_too_large_is_counted_and_passed_over() {
    let target = docs().await;
    let mut lookup =
        super::super::fixture::walk_lookup(target.path(), target.path().join("doc"), 100).await;
    let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
    // The index of each crate rustdoc wrote is past 100 bytes: none is read, and
    // each is counted.
    assert!(catalog.names().is_empty());
    assert_eq!(lookup.reads().too_large, 2);
}

#[cfg(unix)]
#[tokio::test]
async fn a_directory_that_cannot_be_listed_is_counted_and_passed_over() {
    use std::os::unix::fs::PermissionsExt;
    let target = docs().await;
    let locked = target.path().join("doc/demo/inner");
    tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
        .await
        .unwrap();
    // Run as a user every directory opens to (root), there is nothing to test.
    let opens = tokio::fs::read_dir(&locked).await.is_ok();
    let mut lookup = lookup_at(target.path()).await;
    let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
    tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
        .await
        .unwrap();
    if opens {
        return;
    }
    assert_eq!(catalog.unreadable(), 1);
    let names = texts(&catalog);
    assert!(names.contains(&"demo::Shelf".to_owned()));
    assert!(!names.contains(&"demo::inner".to_owned()));
    assert!(!names.contains(&"demo::inner::helper".to_owned()));
}

#[cfg(unix)]
#[tokio::test]
async fn a_path_through_a_symbolic_link_out_of_the_docs_is_no_name() {
    let target = docs().await;
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "mod/index.html", &index(&[])).await;
    write(outside.path(), "mod/fn.escaped.html", &page(&[])).await;
    write(outside.path(), "struct.Esc.html", &page(&[])).await;
    tokio::fs::symlink(
        outside.path().join("mod"),
        target.path().join("doc/demo/out"),
    )
    .await
    .unwrap();
    tokio::fs::symlink(
        outside.path().join("struct.Esc.html"),
        target.path().join("doc/demo/struct.Esc.html"),
    )
    .await
    .unwrap();
    let mut lookup = lookup_at(target.path()).await;
    let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
    let names = texts(&catalog);
    for escaped in ["demo::out", "demo::out::escaped", "demo::Esc"] {
        assert!(!names.contains(&escaped.to_owned()), "{escaped}");
    }
    assert!(names.contains(&"demo::Shelf".to_owned()));
    for (name, place) in catalog.names() {
        assert_eq!(lookup.find(name).await, Found::There(place.clone()));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn an_item_whose_name_is_also_an_entry_out_of_the_docs_is_no_name() {
    let target = docs().await;
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "index.html", &index(&[])).await;
    write(target.path(), "doc/demo/fn.foo.html", &page(&[])).await;
    tokio::fs::symlink(outside.path(), target.path().join("doc/demo/foo"))
        .await
        .unwrap();
    let mut lookup = lookup_at(target.path()).await;
    let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
    let names = texts(&catalog);
    assert!(!names.contains(&"demo::foo".to_owned()));
    assert!(names.contains(&"demo::Shelf".to_owned()));
    for (name, place) in catalog.names() {
        assert_eq!(lookup.find(name).await, Found::There(place.clone()));
    }
}
