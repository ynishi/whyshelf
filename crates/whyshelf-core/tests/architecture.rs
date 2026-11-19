//! Runs the architecture rules of the workspace root's `arch-lint.toml`.

arch_lint::check!();

/// The rules of the artifacts' ports (`ports-by-the-dispatch`, `registry-holds-no-rule`)
/// catch what they are written for, run as `arch-lint.toml` declares them over a tree
/// of fixtures written to a directory of its own (so no file of the workspace holds a
/// violation): a `use`, a path written in full with no `use`, and a `#[cfg(test)]`
/// module of a file that is no file of tests; and they leave out a file of tests
/// (`tests.rs`, `*_tests.rs`) and a line allowed with a reason. They do not see a path
/// through an alias of a module or a glob import: a review reads those.
#[test]
#[arch_lint::allow(
    no_sync_io,
    reason = "a test with no async runtime writes its fixtures; whyshelf-core has no I/O crate"
)]
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
    let root = std::env::temp_dir().join(format!(
        "whyshelf-arch-lint-fixtures-{}",
        std::process::id()
    ));
    for (path, text) in fixtures {
        let file = root.join(INFRA).join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let config =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../arch-lint.toml"))
            .unwrap();
    let result = arch_lint::Session::from_config_str(&root, &config)
        .unwrap()
        .run();
    std::fs::remove_dir_all(&root).unwrap();
    let violations = result.unwrap().violations;
    let caught = |rule: &str| -> Vec<String> {
        let mut at: Vec<String> = violations
            .iter()
            .filter(|v| v.rule == rule)
            .map(|v| format!("{}:{}", v.location.file.display(), v.location.line))
            .collect();
        at.sort();
        at
    };
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
