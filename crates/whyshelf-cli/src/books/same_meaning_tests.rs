//! Tests of the arguments of `judge same`: what clap takes, and what `judge_run`
//! makes of them.

use clap::Parser;

use crate::args::Cli;

use super::*;

const HASH: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000001";

fn verb(args: &[&str]) -> Result<Verb, clap::Error> {
    let cli = Cli::try_parse_from([&["whyshelf", "judge", "same"], args].concat())?;
    match cli.command {
        BooksCommand::Judge(verb) => Ok(verb),
        other => panic!("a judge command: {other:?}"),
    }
}

fn requests(args: &[&str]) -> Result<Vec<SameMeaningRequest>, String> {
    match judge_run(verb(args).expect("the arguments parse"))? {
        JudgeRun::Same(requests) => Ok(requests),
        _ => panic!("a declaration"),
    }
}

#[test]
fn a_file_alone_is_enough_to_name() {
    let requests = requests(&["--artifact", "tests/a.rs", "--reason", "formatting"]).unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        matches!(&requests[0].about, SameMeaningAbout::Artifact(p) if p.as_str() == "tests/a.rs")
    );
    assert_eq!(requests[0].from, None);
}

#[test]
fn nothing_named_or_no_reason_is_refused_by_the_parser() {
    assert!(verb(&["--reason", "typo"]).is_err());
    assert!(verb(&["FR-1"]).is_err());
}

#[test]
fn from_names_the_earlier_hash_of_one_target_only() {
    let one = requests(&["FR-1", "--from", HASH, "--reason", "typo"]).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].from.map(|h| h.to_string()).as_deref(), Some(HASH));

    for args in [
        &["FR-1", "FR-2", "--from", HASH, "--reason", "typo"][..],
        &[
            "FR-1",
            "--artifact",
            "tests/a.rs",
            "--from",
            HASH,
            "--reason",
            "typo",
        ][..],
    ] {
        let err = requests(args).unwrap_err();
        assert!(err.contains("--from"), "{args:?}: {err}");
    }
    let err = requests(&["FR-1", "--from", "v1", "--reason", "typo"]).unwrap_err();
    assert!(err.contains("--from `v1`"), "{err}");
}
