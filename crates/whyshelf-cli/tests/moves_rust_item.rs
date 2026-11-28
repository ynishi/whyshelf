//! `moves` on the binary, for the space `rust-item`: a module `old` that became `new`
//! has its items found at their new paths (by the end of the names, as the docs keep
//! no history) and the move of the prefix once for the names that share it; when a
//! part of `old` is still where it was, the move of the prefix is told as refused, and
//! `judge moved --prefix` refuses it on the same tree; a module moved under another
//! parent, with a name left in the parent, has the move of the parent refused and the
//! move of the module suggested and taken, for the module itself (the move of its whole
//! name) and for the names in it; when no name ends as a gone one does, there
//! is no candidate.
//!
//! The docs are a small tree of HTML laid out as rustdoc writes it, with the page of
//! the source of the crate whose source is at the root of the project, in a build
//! directory `CARGO_TARGET_DIR` names, so no crate is built and no program but the
//! binary is run.

mod common;

use std::fs;
use std::process::Output;

use common::{Project, json};

const BOOKS: &str = r#"[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "text"
required = true
[[book.link]]
name = "items"
target = "artifact"
preset = "rustdoc"
"#;

const GENERATOR: &str = "<meta name=\"generator\" content=\"rustdoc\">";

/// The source of the crate `demo` the docs are of: no character HTML escapes.
const LIB: &str = "//! The demo crate.\n";

/// A note whose link names these paths.
fn note(items: &[&str]) -> String {
    let items: Vec<String> = items.iter().map(|i| format!("\"{i}\"")).collect();
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\nitems: [{}]\n---\nThe body.\n",
        items.join(", ")
    )
}

/// A page of rustdoc.
fn page(text: &str) -> String {
    format!("<!DOCTYPE html><html><head>{GENERATOR}</head><body><p>{text}</p></body></html>")
}

/// The file `at` of the docs of the project (under its build directory), with a page.
fn doc(project: &Project, at: &str) {
    let file = project.0.join("build/doc").join(at);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, page(at)).unwrap();
}

/// The binary run on the project, with the build directory named as cargo does.
fn run(project: &Project, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .arg(&project.0)
        .args(args)
        .env("WHYSHELF_ACTOR", "tester")
        .env("CARGO_TARGET_DIR", project.0.join("build"))
        .output()
        .expect("the binary runs")
}

/// The crate `demo` the docs are of, at the root of the project, and its page of the
/// source: the docs are of the sources as they are.
fn sourced(project: &Project) {
    fs::write(project.0.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    fs::create_dir_all(project.0.join("src")).unwrap();
    fs::write(project.0.join("src/lib.rs"), LIB).unwrap();
    let source = project.0.join("build/doc/src/demo/lib.rs.html");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(
        source,
        format!("<html><head>{GENERATOR}</head><body><pre class=\"rust\"><code>{LIB}</code></pre></body></html>"),
    )
    .unwrap();
}

/// A project whose note names these items of the crate `demo`, all of them in its
/// module `old` (`old/struct.Foo.html` for `demo::old::Foo`, `old/fn.bar.html` for
/// `demo::old::bar`, `old/fn.keep.html` for `demo::old::keep`), approved.
fn approved(name: &str, items: &[&str]) -> Project {
    let project = Project::with(name, BOOKS, &note(items));
    sourced(&project);
    doc(&project, "demo/index.html");
    doc(&project, "demo/old/index.html");
    for item in items {
        doc(&project, &page_of("old", item));
    }
    let out = run(&project, &["judge", "approve", "N-1"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    project
}

/// The page of an item `demo::<module>::<name>`: a struct when its name starts with a
/// capital, else a function.
fn page_of(module: &str, item: &str) -> String {
    let name = item.rsplit("::").next().unwrap();
    let kind = if name.starts_with(char::is_uppercase) {
        "struct"
    } else {
        "fn"
    };
    format!("demo/{module}/{kind}.{name}.html")
}

/// `moves`, checked to exit 0 and to write nothing, its answer.
fn moves(project: &Project) -> serde_json::Value {
    let before = project.files();
    let out = run(project, &["moves"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(project.files(), before, "moves wrote something");
    let answer = json(&out);
    assert_eq!(answer["version"], 1);
    answer
}

/// The gone name `name` of the answer.
fn gone(answer: &serde_json::Value, name: &str) -> serde_json::Value {
    answer["absent"]
        .as_array()
        .unwrap()
        .iter()
        .find(|one| one["name"] == name)
        .unwrap_or_else(|| panic!("{name} is not gone: {answer}"))
        .clone()
}

/// The module `old` taken away, and a module `new` with these of its items written.
fn renamed(project: &Project, items: &[&str]) {
    fs::remove_dir_all(project.0.join("build/doc/demo/old")).unwrap();
    doc(project, "demo/new/index.html");
    for item in items {
        doc(project, &page_of("new", item));
    }
}

#[test]
fn a_module_that_became_another_has_its_items_at_their_new_paths_and_one_prefix() {
    let project = approved(
        "moves-rust-item-module",
        &["demo::old::Foo", "demo::old::bar"],
    );
    renamed(&project, &["demo::new::Foo", "demo::new::bar"]);

    let answer = moves(&project);
    assert_eq!(answer["cannot_suggest"], serde_json::json!([]));
    for (old, new) in [
        ("demo::old::Foo", "demo::new::Foo"),
        ("demo::old::bar", "demo::new::bar"),
    ] {
        let one = gone(&answer, old);
        assert_eq!(one["space"], "rust-item");
        // The docs keep no history, and the preset compares presence: the end of the
        // name is all there is, said so.
        assert_eq!(
            one["candidates"],
            serde_json::json!([
                {"name": new, "found": "same_end", "similarity": null, "present": true}
            ])
        );
        assert_eq!(one["notes"], serde_json::json!([]));
    }
    // One move of a prefix, for the two names that share it, and none refused.
    assert_eq!(
        answer["prefixes"],
        serde_json::json!([{
            "from": "demo::old",
            "to": "demo::new",
            "moved": [
                {"space": "rust-item", "name": "demo::old::Foo"},
                {"space": "rust-item", "name": "demo::old::bar"},
            ],
            "not_asked": []
        }])
    );
    assert_eq!(answer["refused_prefixes"], serde_json::json!([]));
    assert_eq!(answer["untried_prefixes"], 0);
    let drawing = answer["drawing"].as_str().unwrap();
    assert!(drawing.contains("-> demo::new::Foo  same_end"), "{drawing}");
    assert!(
        drawing.contains("prefix demo::old -> demo::new (2 names)"),
        "{drawing}"
    );

    // What it suggests is what `judge moved --prefix` takes.
    let out = run(
        &project,
        &[
            "judge",
            "moved",
            "--prefix",
            "demo::old",
            "demo::new",
            "--reason",
            "the module was renamed",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(moves(&project)["absent"], serde_json::json!([]));
}

#[test]
fn a_prefix_with_a_name_left_in_the_old_module_is_refused_by_moves_and_by_judge_moved() {
    let project = approved(
        "moves-rust-item-left",
        &["demo::old::Foo", "demo::old::bar", "demo::old::keep"],
    );
    // `Foo` and `bar` moved to `new`; `keep` is still in `old`, at its old name.
    renamed(&project, &["demo::new::Foo", "demo::new::bar"]);
    doc(&project, "demo/old/index.html");
    doc(&project, &page_of("old", "demo::old::keep"));

    let answer = moves(&project);
    // Both gone names still have their candidate.
    assert_eq!(
        gone(&answer, "demo::old::Foo")["candidates"][0]["name"],
        "demo::new::Foo"
    );
    assert_eq!(
        gone(&answer, "demo::old::bar")["candidates"][0]["name"],
        "demo::new::bar"
    );
    // The move of the prefix is not suggested: it is told, as refused, for the one name.
    assert_eq!(answer["prefixes"], serde_json::json!([]));
    assert_eq!(
        answer["refused_prefixes"],
        serde_json::json!([{"from": "demo::old", "to": "demo::new", "refused": 1}])
    );
    let drawing = answer["drawing"].as_str().unwrap();
    assert!(
        drawing.contains("refused prefix demo::old -> demo::new (1 name cannot be moved)"),
        "{drawing}"
    );

    // On this tree, `judge moved --prefix` refuses it, by name, and writes nothing.
    let before = project.files();
    let out = run(
        &project,
        &[
            "judge",
            "moved",
            "--prefix",
            "demo::old",
            "demo::new",
            "--reason",
            "the module was renamed",
        ],
    );
    assert_eq!(out.status.code(), Some(1), "{}", json(&out));
    let error = json(&out);
    assert_eq!(error["error"], "refused");
    let refused = error["refused"].as_array().unwrap();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["name"], "demo::old::keep");
    assert_eq!(refused[0]["space"], "rust-item");
    assert_eq!(project.files(), before, "a refused move writes no record");
}

#[test]
fn a_module_moved_under_another_parent_with_a_sibling_left_has_the_narrow_prefix_that_stands() {
    let project = Project::with(
        "moves-rust-item-parent",
        BOOKS,
        &note(&["demo::app::x", "demo::app::x::Foo", "demo::app::keep"]),
    );
    sourced(&project);
    for at in [
        "demo/index.html",
        "demo/app/index.html",
        "demo/app/x/index.html",
        "demo/app/x/struct.Foo.html",
        "demo/app/fn.keep.html",
    ] {
        doc(&project, at);
    }
    let out = run(&project, &["judge", "approve", "N-1"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The module `x` is now under `domain` and keeps its name; `keep` stays in `app`.
    fs::remove_dir_all(project.0.join("build/doc/demo/app/x")).unwrap();
    for at in [
        "demo/domain/index.html",
        "demo/domain/x/index.html",
        "demo/domain/x/struct.Foo.html",
    ] {
        doc(&project, at);
    }

    let answer = moves(&project);
    // The module itself is pinned too, and is gone with what was in it: the one name in
    // it would be the one holder of the move of the module, if the module's own
    // candidate did not carry the move of its whole name too.
    for (old, new) in [
        ("demo::app::x", "demo::domain::x"),
        ("demo::app::x::Foo", "demo::domain::x::Foo"),
    ] {
        assert_eq!(
            gone(&answer, old)["candidates"],
            serde_json::json!([
                {"name": new, "found": "same_end", "similarity": null, "present": true}
            ])
        );
    }
    // The move of the parent is refused for `keep`; the move of the module, which is
    // the move of the whole name of the module itself, stands for the module and what
    // was in it.
    assert_eq!(
        answer["prefixes"],
        serde_json::json!([{
            "from": "demo::app::x",
            "to": "demo::domain::x",
            "moved": [
                {"space": "rust-item", "name": "demo::app::x"},
                {"space": "rust-item", "name": "demo::app::x::Foo"},
            ],
            "not_asked": []
        }])
    );
    assert_eq!(
        answer["refused_prefixes"],
        serde_json::json!([{"from": "demo::app", "to": "demo::domain", "refused": 1}])
    );
    let drawing = answer["drawing"].as_str().unwrap();
    assert!(
        drawing.contains("prefix demo::app::x -> demo::domain::x (2 names)"),
        "{drawing}"
    );
    assert!(
        drawing.contains("refused prefix demo::app -> demo::domain (1 name cannot be moved)"),
        "{drawing}"
    );

    // `judge moved --prefix` refuses the wide one, for `keep`, and writes nothing.
    let before = project.files();
    let out = run(
        &project,
        &[
            "judge",
            "moved",
            "--prefix",
            "demo::app",
            "demo::domain",
            "--reason",
            "the module was moved",
        ],
    );
    assert_eq!(out.status.code(), Some(1), "{}", json(&out));
    let refused = json(&out)["refused"].as_array().unwrap().clone();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["name"], "demo::app::keep");
    assert_eq!(project.files(), before, "a refused move writes no record");

    // And it takes the narrow one, which is what `moves` suggested.
    let out = run(
        &project,
        &[
            "judge",
            "moved",
            "--prefix",
            "demo::app::x",
            "demo::domain::x",
            "--reason",
            "the module was moved",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(moves(&project)["absent"], serde_json::json!([]));
}

#[test]
fn when_no_name_ends_as_a_gone_one_does_there_is_no_candidate() {
    let project = approved(
        "moves-rust-item-none",
        &["demo::old::Foo", "demo::old::bar"],
    );
    renamed(&project, &["demo::new::Baz"]);

    let answer = moves(&project);
    for old in ["demo::old::Foo", "demo::old::bar"] {
        let one = gone(&answer, old);
        assert_eq!(one["candidates"], serde_json::json!([]), "{old}");
        assert_eq!(one["notes"], serde_json::json!([]), "{old}");
    }
    assert_eq!(answer["prefixes"], serde_json::json!([]));
    assert_eq!(answer["refused_prefixes"], serde_json::json!([]));
    assert!(
        answer["drawing"]
            .as_str()
            .unwrap()
            .contains("(no candidate)")
    );
}
