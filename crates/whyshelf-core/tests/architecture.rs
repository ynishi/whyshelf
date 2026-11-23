//! Runs the architecture rules of the workspace root's `arch-lint.toml`.

arch_lint::check!();

/// Runs the rules as `arch-lint.toml` declares them over a tree of fixtures written to
/// a directory of its own (`name` and the process id, under the temporary directory),
/// each `(path, text)` a file of it, and gives where each rule caught something: a
/// function of the rule's name to its `file:line`s, sorted. The tree is removed before
/// the answer of the run is read, so a run that fails leaves nothing behind.
#[arch_lint::allow(
    no_sync_io,
    reason = "a test with no async runtime writes its fixtures; whyshelf-core has no I/O crate"
)]
fn caught_over(name: &str, files: &[(String, &str)]) -> impl Fn(&str) -> Vec<String> {
    let root = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    for (path, text) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let config =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../arch-lint.toml"))
            .unwrap();
    let result = arch_lint::Session::from_config_str(&root, &config).map(|session| session.run());
    std::fs::remove_dir_all(&root).unwrap();
    let violations = result.unwrap().unwrap().violations;
    move |rule: &str| {
        let mut at: Vec<String> = violations
            .iter()
            .filter(|v| v.rule == rule)
            .map(|v| format!("{}:{}", v.location.file.display(), v.location.line))
            .collect();
        at.sort();
        at
    }
}

/// The rules of the artifacts' ports (`ports-by-the-dispatch`, `registry-holds-no-rule`)
/// catch what they are written for, run as `arch-lint.toml` declares them over a tree
/// of fixtures written to a directory of its own (so no file of the workspace holds a
/// violation): a `use`, a path written in full with no `use`, and a `#[cfg(test)]`
/// module of a file that is no file of tests; and they leave out a file of tests
/// (`tests.rs`, `*_tests.rs`) and a line allowed with a reason. They do not see a path
/// through an alias of a module or a glob import: a review reads those.
#[test]
fn the_rules_of_the_artifacts_ports_catch_what_they_are_written_for() {
    const INFRA: &str = "crates/whyshelf-infra/src/shelf";
    let fixtures = [
        (
            "artifact/probe.rs",
            "struct X;\n\
             impl whyshelf_core::app::shelf::ArtifactSource for X {}\n\
             impl whyshelf_core::app::shelf::ports::MoveSource for X {}\n\
             #[cfg(test)]\n\
             mod tests {\n    use whyshelf_core::app::shelf::MoveSource;\n}\n\
             // arch-lint: allow(ports-by-the-dispatch) reason=\"a fixture of an allowed line\"\n\
             use whyshelf_core::app::shelf::ArtifactSource;\n",
        ),
        (
            "artifact/probe_tests.rs",
            "use whyshelf_core::app::shelf::ArtifactSource;\n",
        ),
        (
            "init/tests.rs",
            "use whyshelf_core::app::shelf::MoveSource;\n",
        ),
        (
            "artifact/registry.rs",
            "fn answer() {\n    \
             let _ = whyshelf_core::domain::shelf::artifact::ArtifactReading::absent();\n    \
             let _ = whyshelf_core::domain::shelf::artifact::SpacePrefixes::NoPrefixes;\n}\n\
             use whyshelf_core::domain::shelf::ledger::SpaceSuggestions;\n",
        ),
    ];
    let files: Vec<(String, &str)> = fixtures
        .into_iter()
        .map(|(path, text)| (format!("{INFRA}/{path}"), text))
        .collect();
    let caught = caught_over("whyshelf-arch-lint-fixtures", &files);
    assert_eq!(
        caught("ports-by-the-dispatch"),
        [
            format!("{INFRA}/artifact/probe.rs:2"),
            format!("{INFRA}/artifact/probe.rs:3"),
            format!("{INFRA}/artifact/probe.rs:6"),
        ]
    );
    assert_eq!(
        caught("registry-holds-no-rule"),
        [
            format!("{INFRA}/artifact/registry.rs:2"),
            format!("{INFRA}/artifact/registry.rs:3"),
            format!("{INFRA}/artifact/registry.rs:5"),
        ]
    );
}

/// The rule `ledger-reads-only-its-ground` catches what it is written for, run as
/// `arch-lint.toml` declares it over a tree of fixtures written to a directory of its
/// own. The ledger of the tree reads nothing it denies, so the rule passes there
/// without showing that it works: here it is shown to catch a `use` of a module that
/// reads the ledger (as `super::super::X` from a file of the ledger's directory, or as
/// `super::X` from `ledger.rs`), a path written in full in code, a module of the shelf
/// no list names (a rule written from the side that is denied would let it through) and
/// a glob import of the shelf (`super::super::*`, whose candidate files are all of
/// `shelf/*.rs`); and to leave alone a file of tests (`tests.rs`, `*_tests.rs`), a link
/// of a doc comment (which is no path to the rule), a module of the ledger's own
/// ground (`schema`, `drift`) and the ledger's own children.
///
/// It does not see a path inside the arguments of a macro (`matches!(d,
/// super::super::cover::X)`: tokens, not a path), a path through an alias of a module
/// (`use super::super as shelf;` and then `shelf::freshness::X`), a `pub use`
/// re-export or a bare path. A review reads those. It also forbids a path of
/// `shelf/mod.rs` to its own items, a re-export from an allowed module included,
/// because the candidate file of `super::super::X` is `shelf/X.rs`: the safe side, and
/// the ledger reads no such path now.
///
/// `super::` is resolved from the module path of the file in its package, which is the
/// directory of a `Cargo.toml` with a `[package]` table: the fixture writes one.
#[test]
fn the_rule_of_the_ledgers_ground_catches_what_it_is_written_for() {
    const SHELF: &str = "crates/whyshelf-core/src/domain/shelf";
    let fixtures = [
        ("ledger/frame.rs", "use super::super::cover::Decided;\n"),
        ("ledger.rs", "use super::freshness::Standing;\n"),
        (
            "ledger/readers.rs",
            "fn f() { let _ = super::super::freshness::assess; }\n",
        ),
        (
            "ledger/exempt.rs",
            "/// [x](super::super::exemption::frame_exempt)\nfn g() {}\n",
        ),
        ("ledger/pin.rs", "use super::super::stats::Stats;\n"),
        ("ledger/canon.rs", "use super::super::*;\n"),
        (
            "ledger/x_tests.rs",
            "use super::super::freshness::assess;\n",
        ),
        ("ledger/tests.rs", "use super::super::freshness::assess;\n"),
        ("ledger/admit.rs", "use super::super::schema::BookName;\n"),
        ("ledger/compared.rs", "use super::super::drift::Edit;\n"),
        ("ledger/record.rs", "use super::Decision;\n"),
        (
            "ledger/decision.rs",
            "fn h(d: u8) -> bool { matches!(d, super::super::cover::X) }\n",
        ),
    ];
    let package = (
        "crates/whyshelf-core/Cargo.toml".to_owned(),
        "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\n",
    );
    let files: Vec<(String, &str)> = std::iter::once(package)
        .chain(
            fixtures
                .into_iter()
                .map(|(path, text)| (format!("{SHELF}/{path}"), text)),
        )
        .collect();
    let caught = caught_over("whyshelf-arch-lint-ledger-fixtures", &files);
    assert_eq!(
        caught("ledger-reads-only-its-ground"),
        [
            format!("{SHELF}/ledger.rs:1"),
            format!("{SHELF}/ledger/canon.rs:1"),
            format!("{SHELF}/ledger/frame.rs:1"),
            format!("{SHELF}/ledger/pin.rs:1"),
            format!("{SHELF}/ledger/readers.rs:1"),
        ]
    );
}

/// The rule `build-reads-not-footing-or-reach` catches what it is written for, run as
/// `arch-lint.toml` declares it over a tree of fixtures written to a directory of its
/// own, as the ledger's rule above is: a `use` of footing, a path to reach written in
/// full in code, and a `use` through `crate::` from a file under `build/`; and it
/// leaves alone a module the build may read (`doc`) and a file of tests
/// (`*_tests.rs`). What it does not see is what the ledger's rule does not, and a
/// review reads those.
#[test]
fn the_rule_of_the_builds_ground_catches_what_it_is_written_for() {
    const SHELF: &str = "crates/whyshelf-core/src/domain/shelf";
    let fixtures = [
        (
            "build.rs",
            "use super::footing::Footing;\n\
             fn f() { let _ = super::reach::walk; }\n\
             use super::doc::Doc;\n",
        ),
        (
            "build/plan.rs",
            "use crate::domain::shelf::footing::Footing;\n\
             use super::super::doc::LinkState;\n",
        ),
        (
            "build/plan_tests.rs",
            "use super::super::super::reach::walk;\n",
        ),
    ];
    let package = (
        "crates/whyshelf-core/Cargo.toml".to_owned(),
        "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\n",
    );
    let files: Vec<(String, &str)> = std::iter::once(package)
        .chain(
            fixtures
                .into_iter()
                .map(|(path, text)| (format!("{SHELF}/{path}"), text)),
        )
        .collect();
    let caught = caught_over("whyshelf-arch-lint-build-fixtures", &files);
    assert_eq!(
        caught("build-reads-not-footing-or-reach"),
        [
            format!("{SHELF}/build.rs:1"),
            format!("{SHELF}/build.rs:2"),
            format!("{SHELF}/build/plan.rs:1"),
        ]
    );
}

/// The rule `drift-reads-only-its-ground` catches what it is written for, run as
/// `arch-lint.toml` declares it over a tree of fixtures written to a directory of its
/// own. drift reads nothing it denies, so the rule passes in the tree without showing
/// that it works: here it is shown to catch a `use` of the ledger (`super::ledger::X`),
/// of a module that reads the ledger (stats), a path written in full in code
/// (freshness) and a `use` through `crate::` (snapshot); and to leave alone a module of
/// its own ground (`doc`, `artifact`) and a file of tests (`tests.rs`), which reads the
/// ledger to build its cases. What it does not see is what the ledger's rule does not
/// (a path inside a macro, a module through an alias, a re-export), and a review reads
/// those.
///
/// The fixture writes a `Cargo.toml` with a `[package]` table, as `super::` is resolved
/// from the module path of a file in its package.
#[test]
fn the_rule_of_drifts_ground_catches_what_it_is_written_for() {
    const SHELF: &str = "crates/whyshelf-core/src/domain/shelf";
    let fixtures = [
        (
            "drift.rs",
            "use super::ledger::JudgementId;\n\
             use super::stats::Stats;\n\
             fn f() { let _ = super::freshness::assess; }\n\
             use crate::domain::shelf::snapshot::ItemCompared;\n\
             use super::doc::DocHash;\n\
             use super::artifact::Comparison;\n",
        ),
        ("drift/tests.rs", "use super::super::ledger::JudgementId;\n"),
    ];
    let package = (
        "crates/whyshelf-core/Cargo.toml".to_owned(),
        "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\n",
    );
    let files: Vec<(String, &str)> = std::iter::once(package)
        .chain(
            fixtures
                .into_iter()
                .map(|(path, text)| (format!("{SHELF}/{path}"), text)),
        )
        .collect();
    let caught = caught_over("whyshelf-arch-lint-drift-fixtures", &files);
    assert_eq!(
        caught("drift-reads-only-its-ground"),
        [
            format!("{SHELF}/drift.rs:1"),
            format!("{SHELF}/drift.rs:2"),
            format!("{SHELF}/drift.rs:3"),
            format!("{SHELF}/drift.rs:4"),
        ]
    );
}
