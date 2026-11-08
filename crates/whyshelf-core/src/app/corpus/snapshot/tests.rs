//! The read every use case starts from keeps what it was told, and writes nothing.

use super::*;
use crate::app::corpus::fixtures::{DESIGN, FINDING, ITEM, World, by_id, item_one, rel};
use crate::app::fake::block_on;
use crate::app::outcome::{Miss, Refusal};
use crate::app::ports::{PathProblem, RecordProblem};
use crate::domain::corpus::{GitPlace, Visibility};
use crate::domain::id::test_support::docid;
use crate::domain::judgement::test_support::jid;
use crate::domain::naming::UnresolvedWhy;

fn read(world: &World) -> Snapshot {
    block_on(Snapshot::read(&world.fake, world.config.gate_policy())).expect("a snapshot")
}

#[test]
fn the_snapshot_holds_the_documents_with_the_place_git_makes_of_each() {
    let world = World::project(Visibility::Public);
    world
        .fake
        .set_place(Visibility::Public, &rel("finding.md"), GitPlace::Ignored);
    let snapshot = read(&world);
    assert_eq!(snapshot.corpus().documents().len(), 3);
    let finding = snapshot.corpus().by_id(&docid(FINDING));
    assert_eq!(finding.len(), 1);
    assert_eq!(
        finding[0].location().git(),
        Knowable::Known(GitPlace::Ignored)
    );
    // The adapter states no place: the snapshot asked the revision port.
    let design = snapshot.corpus().by_id(&docid(DESIGN));
    assert_eq!(
        design[0].location().git(),
        Knowable::Known(GitPlace::Tracked)
    );
    assert!(snapshot.evaluation().document(&docid(FINDING)).is_some());
}

#[test]
fn what_could_not_be_read_is_kept_beside_the_corpus() {
    let world = World::project(Visibility::Private);
    world
        .fake
        .add_unreadable_path("notes/\u{fffd}.md", PathProblem::NotUtf8);
    let bad = crate::app::ports::CorruptRecord::new(jid(5), RecordProblem::Undecodable);
    world.fake.seed_corrupt(Visibility::Private, bad);
    world
        .fake
        .set_private_root(crate::domain::boundary::PrivateRoot::Absent);
    let snapshot = read(&world);
    assert_eq!(snapshot.unreadable().len(), 1);
    assert_eq!(snapshot.corrupt(), [(Visibility::Private, bad)]);
    assert_eq!(
        snapshot.private_root(),
        crate::domain::boundary::PrivateRoot::Absent
    );
}

#[test]
fn keys_that_cannot_be_looked_for_here_are_an_empty_keyring() {
    let world = World::project(Visibility::Public);
    world.fake.set_keys(Knowable::Unknown);
    let snapshot = read(&world);
    assert_eq!(snapshot.corpus().documents().len(), 3);
    assert_eq!(snapshot.policy(), world.config.gate_policy());
}

#[test]
fn the_text_of_a_version_is_kept_for_the_secret_scan() {
    let world = World::project(Visibility::Public);
    let snapshot = read(&world);
    let file = &snapshot.corpus().by_id(&docid(FINDING))[0];
    let text = snapshot.text(file.text()).expect("the text was read");
    assert_eq!(text.text_hash(), file.text());

    let unknown = TextHash::of("a text no file has");
    assert_eq!(
        snapshot.text(unknown).unwrap_err(),
        ChainError::TextNotRead { text: unknown }
    );
}

#[test]
fn a_name_is_looked_up_in_the_corpus() {
    let world = World::project(Visibility::Public);
    let snapshot = read(&world);
    assert_eq!(
        snapshot.named(&by_id(FINDING)),
        Ok(Named::Doc(docid(FINDING)))
    );
    // An item is found in its Design, and belongs to it.
    let item = snapshot.named(&item_one()).expect("the item");
    match &item {
        Named::Item(held) => {
            assert_eq!(held.design(), docid(DESIGN));
            assert_eq!(held.id(), crate::domain::id::test_support::iid(ITEM));
        }
        Named::Doc(_) => panic!("an item was expected"),
    }
    assert_eq!(crate::app::corpus::home_of(&item), docid(DESIGN));
}

#[test]
fn a_name_that_matches_nothing_says_that_no_history_was_read() {
    let world = World::project(Visibility::Public);
    let snapshot = read(&world);
    // The histories that would explain a rename are not read by any port: for a name
    // that can be renamed the hint is unknown, which is not the statement that no
    // earlier name fits.
    let alias: Ref = "spec/gone".parse().expect("a reference");
    assert_eq!(
        snapshot.named(&alias),
        Err(Outcome::NotFound {
            query: alias,
            miss: Miss::Unmatched {
                hint: Knowable::Unknown
            }
        })
    );
    // An identifier is never renamed, so for it the lookup does know that none fits.
    let missing = by_id(999);
    assert_eq!(
        snapshot.named(&missing),
        Err(Outcome::NotFound {
            query: missing,
            miss: Miss::Unmatched {
                hint: Knowable::Known(None)
            }
        })
    );
}

#[test]
fn a_private_name_that_names_nothing_here_cannot_be_looked_up() {
    let world = World::project(Visibility::Public);
    let snapshot = read(&world);
    let query: Ref = "private:0192f3a4-5b6c-7d8e-8f90-000000000001"
        .parse()
        .expect("a reference");
    assert_eq!(
        snapshot.named(&query),
        Err(Outcome::Refused(Refusal::CannotNameHere {
            query,
            why: UnresolvedWhy::PrivateNotHere,
        }))
    );
}

#[test]
fn a_port_that_fails_is_an_error_naming_the_port() {
    for port in [
        PortName::Documents,
        PortName::Ledger,
        PortName::Keys,
        PortName::RevisionFacts,
    ] {
        let world = World::project(Visibility::Public);
        world.fake.fail(port);
        match block_on(Snapshot::read(&world.fake, world.config.gate_policy())) {
            Err(AppError::Port { port: failed, .. }) => assert_eq!(failed, port),
            other => panic!("the {port} port was made to fail: {other:?}"),
        }
    }
}

#[test]
fn status_gives_the_snapshot_and_calls_no_write() {
    let world = World::project(Visibility::Public);
    let outcome = block_on(status(&world.fake, &world.config)).expect("an outcome");
    let Outcome::Done(snapshot) = outcome else {
        panic!("status did not come to done");
    };
    assert_eq!(snapshot.corpus().documents().len(), 3);
    assert_eq!(world.fake.writes(), 0);
}
