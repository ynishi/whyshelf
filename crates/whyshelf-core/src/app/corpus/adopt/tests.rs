//! Taking a decision in, and giving files the identity they lack, through the fake
//! ports.

use super::*;
use crate::app::corpus::Snapshot;
use crate::app::corpus::fixtures::{
    DESIGN, FINDING, ITEM, SPEC, World, by_id, edited, item_one, place, raw_document, rel, text_of,
};
use crate::app::fake::{Reply, block_on};
use crate::app::outcome::{ExitClass, exit_class};
use crate::app::ports::{ConfirmAction, Confirmed, PortName};
use crate::domain::corpus::Entity;
use crate::domain::design::test_support::{design, item};
use crate::domain::error::AdoptionRefusal;
use crate::domain::finding::test_support::finding;
use crate::domain::id::test_support::{docid, iid};
use crate::domain::judge::test_support::agent;
use crate::domain::judgement::Basis;
use crate::domain::knowable::Knowable;
use crate::domain::location::RelPath;
use crate::domain::publication::ConfirmDigest;

type Run<T> = Result<Outcome<T>, AppError>;

const SOURCE: &str = "the old tracker";

fn adopt_run(world: &World, gate: Gate, target: Ref, source: &str) -> Run<Adopted> {
    block_on(adopt(
        &world.fake,
        &world.config,
        Adopt {
            gate,
            target,
            source: source.to_owned(),
        },
    ))
}

fn import_run(world: &World, command: Import) -> Run<Imported> {
    block_on(import(&world.fake, &world.config, command))
}

fn items_of(imported: &Imported) -> &[ItemId] {
    match imported {
        Imported::Items { items, .. } => items,
        other => panic!("an import of items was expected: {other:?}"),
    }
}

fn adopted_of(imported: &Imported) -> Option<&Adopted> {
    match imported {
        Imported::File { adopted, .. } => adopted.as_ref(),
        other => panic!("an import of a file was expected: {other:?}"),
    }
}

fn done<T: std::fmt::Debug>(run: Run<T>) -> T {
    match run.expect("an outcome, not an error") {
        Outcome::Done(value) => value,
        other => panic!("done was expected: {other:?}"),
    }
}

fn refusal_of<T: std::fmt::Debug>(run: Run<T>) -> Refusal {
    match run.expect("an outcome, not an error") {
        Outcome::Refused(refusal) => refusal,
        other => panic!("a refusal was expected: {other:?}"),
    }
}

fn at(root: Visibility, path: &str) -> Location {
    Location::new(root, rel(path), Knowable::Unknown)
}

// --- adopt ------------------------------------------------------------------------

#[test]
fn an_adoption_is_confirmed_by_its_own_digest_and_recorded_as_adopted() {
    let world = World::project(Visibility::Public);
    let adopted = done(adopt_run(&world, Gate::Cr, by_id(FINDING), SOURCE));
    assert_eq!(adopted.gate(), Gate::Cr);
    assert_eq!(adopted.target(), Target::Doc(docid(FINDING)));
    assert_eq!(adopted.ledger(), Visibility::Public);

    // It always asks, about the adoption, and nothing else.
    let asked = world.fake.asked();
    assert_eq!(asked.len(), 1);
    assert!(matches!(asked[0].digest(), ConfirmDigest::Adopt(_)));
    assert_eq!(asked[0].summary().action(), ConfirmAction::Adopt);

    let records = world.fake.records(Visibility::Public);
    assert_eq!(records.len(), 1);
    assert!(matches!(records[0].kind(), JudgementKind::Adopt { .. }));
    let snapshot =
        block_on(Snapshot::read(&world.fake, world.config.gate_policy())).expect("a snapshot");
    let report = snapshot
        .evaluation()
        .document(&docid(FINDING))
        .expect("the finding");
    assert!(
        report
            .bases()
            .iter()
            .any(|basis| matches!(basis, Basis::Adopted { .. }))
    );
}

#[test]
fn an_item_is_adopted_through_its_design() {
    let world = World::project(Visibility::Public);
    let adopted = done(adopt_run(&world, Gate::Ca, item_one(), SOURCE));
    assert_eq!(adopted.target(), Target::Item(iid(ITEM)));
}

#[test]
fn an_agent_may_not_adopt_and_is_not_asked() {
    let world = World::project(Visibility::Public);
    world.fake.set_actor(agent("bot", "tool"));
    assert_eq!(
        refusal_of(adopt_run(&world, Gate::Cr, by_id(FINDING), SOURCE)),
        Refusal::AgentCannotAdopt
    );
    assert!(world.fake.asked().is_empty());
    assert_eq!(world.fake.writes(), 0);
}

#[test]
fn an_adoption_nobody_confirms_is_not_recorded() {
    for (answer, refusal) in [
        (Confirmed::No, Refusal::ConfirmDeclined),
        (Confirmed::Unavailable, Refusal::NeedsHumanConfirm),
    ] {
        let world = World::project(Visibility::Public);
        world.fake.set_reply(Reply::Always(answer));
        assert_eq!(
            refusal_of(adopt_run(&world, Gate::Cr, by_id(FINDING), SOURCE)),
            refusal
        );
        assert_eq!(world.fake.writes(), 0);
    }
}

#[test]
fn a_confirmation_does_not_hold_for_a_file_edited_after_the_question() {
    let world = World::project(Visibility::Public);
    world.fake.on_next_ask(edited(
        Visibility::Public,
        "finding.md",
        Entity::Finding(finding(FINDING, "edited")),
        "edited\n",
    ));
    assert!(matches!(
        refusal_of(adopt_run(&world, Gate::Cr, by_id(FINDING), SOURCE)),
        Refusal::ConfirmationStale { .. }
    ));
    assert_eq!(world.fake.writes(), 0);
}

#[test]
fn what_the_adoption_check_refuses_is_a_refusal_and_asks_nobody() {
    let world = World::project(Visibility::Public);
    let target = Target::Doc(docid(SPEC));
    assert_eq!(
        refusal_of(adopt_run(&world, Gate::Cm, by_id(SPEC), SOURCE)),
        Refusal::Adoption(AdoptionRefusal::GateNotAdoptable { gate: Gate::Cm })
    );
    assert_eq!(
        refusal_of(adopt_run(&world, Gate::Cr, by_id(SPEC), SOURCE)),
        Refusal::Adoption(AdoptionRefusal::GateTargetMismatch {
            gate: Gate::Cr,
            target
        })
    );
    assert!(matches!(
        refusal_of(adopt_run(&world, Gate::Cs, by_id(SPEC), "  ")),
        Refusal::Adoption(AdoptionRefusal::SourceEmpty(_))
    ));
    assert!(world.fake.asked().is_empty());
    assert_eq!(world.fake.writes(), 0);
}

#[test]
fn a_name_that_matches_nothing_is_not_found_and_a_cut_short_publication_holds_it_back() {
    let world = World::project(Visibility::Public);
    let run = adopt_run(&world, Gate::Cr, by_id(999), SOURCE);
    assert_eq!(exit_class(&run), ExitClass::NotFound);

    let world = World::new();
    let entity = Entity::Finding(finding(FINDING, "body"));
    world.add(Visibility::Public, "public.md", entity.clone());
    world.add(Visibility::Private, "private.md", entity);
    assert_eq!(
        refusal_of(adopt_run(&world, Gate::Cr, by_id(FINDING), SOURCE)),
        Refusal::PublishInProgress(docid(FINDING))
    );
}

#[test]
fn a_publication_that_starts_while_the_person_answers_holds_the_adoption_back() {
    let world = World::project(Visibility::Public);
    // The document is copied to the other root after the question and before the
    // snapshot is read again.
    world.fake.on_next_ask(|fake| {
        let entity = Entity::Finding(finding(FINDING, "body"));
        place(
            fake,
            Visibility::Private,
            "private.md",
            raw_document(entity.clone(), &text_of(&entity)),
        );
    });
    assert_eq!(
        refusal_of(adopt_run(&world, Gate::Cr, by_id(FINDING), SOURCE)),
        Refusal::PublishInProgress(docid(FINDING))
    );
    assert_eq!(world.fake.asked().len(), 1);
    assert_eq!(world.fake.writes(), 0);
    assert_eq!(world.records(Visibility::Public), 0);
    assert_eq!(world.records(Visibility::Private), 0);
}

#[test]
fn a_person_who_cannot_be_asked_is_a_failed_port() {
    let world = World::project(Visibility::Public);
    world.fake.fail(PortName::Confirm);
    let run = adopt_run(&world, Gate::Cr, by_id(FINDING), SOURCE);
    assert_eq!(exit_class(&run), ExitClass::Port);
}

#[test]
fn a_secret_in_the_adopted_text_is_not_stored() {
    let world = World::new();
    world.add_text(
        Visibility::Public,
        "finding.md",
        Entity::Finding(finding(FINDING, "body")),
        "AKIA-SECRET\n",
    );
    assert!(matches!(
        refusal_of(adopt_run(&world, Gate::Cr, by_id(FINDING), SOURCE)),
        Refusal::Secret(_)
    ));
    assert_eq!(world.fake.writes(), 0);
}

// --- import: a file ---------------------------------------------------------------

const NEW: &str = "---\nkind: finding\n---\nbody\n";

fn file_import(world: &World, adopt: Option<AdoptAfter>) -> Run<Imported> {
    import_run(
        world,
        Import::File {
            at: at(Visibility::Public, "new.md"),
            adopt,
        },
    )
}

#[test]
fn an_import_writes_the_id_and_changes_nothing_else_and_asks_nobody() {
    let world = World::new();
    world.unidentified(Visibility::Public, "new.md", NEW);
    let imported = done(file_import(&world, None));
    assert!(adopted_of(&imported).is_none());
    assert_eq!(
        world.fake.file(Visibility::Public, &rel("new.md")),
        Some(format!("---\nid: {}\nkind: finding\n---\nbody\n", imported.doc()).into_bytes())
    );
    // Importing takes nothing in, so there is nothing that cannot be undone to confirm.
    assert!(world.fake.asked().is_empty());
}

#[test]
fn an_import_keeps_the_line_ends_of_the_file() {
    let world = World::new();
    world.unidentified(
        Visibility::Public,
        "new.md",
        "---\r\nkind: finding\r\n---\r\nbody\r\n",
    );
    let imported = done(file_import(&world, None));
    assert_eq!(
        world.fake.file(Visibility::Public, &rel("new.md")),
        Some(
            format!(
                "---\r\nid: {}\r\nkind: finding\r\n---\r\nbody\r\n",
                imported.doc()
            )
            .into_bytes()
        )
    );
}

#[test]
fn an_import_that_adopts_asks_about_the_adoption_only() {
    let world = World::new();
    world.unidentified(Visibility::Public, "new.md", NEW);
    // Once the file has its id, a listing made again shows a document under it.
    let minted = docid(1);
    world.fake.on_next_rewrite(move |fake| {
        fake.remove_document(Visibility::Public, &rel("new.md"));
        let entity = Entity::Finding(finding(1, "body"));
        place(
            fake,
            Visibility::Public,
            "new.md",
            raw_document(entity, "identified\n"),
        );
    });
    let imported = done(file_import(
        &world,
        Some(AdoptAfter {
            gate: Gate::Cr,
            source: SOURCE.to_owned(),
        }),
    ));
    assert_eq!(imported.doc(), minted);
    let adopted = adopted_of(&imported).expect("the adoption");
    assert_eq!(adopted.target(), Target::Doc(minted));
    let asked = world.fake.asked();
    assert_eq!(asked.len(), 1);
    assert!(matches!(asked[0].digest(), ConfirmDigest::Adopt(_)));
}

#[test]
fn an_import_that_adopts_keeps_the_id_when_the_adoption_is_refused() {
    let world = World::new();
    world.unidentified(Visibility::Public, "new.md", NEW);
    world.fake.set_reply(Reply::Always(Confirmed::No));
    world.fake.on_next_rewrite(|fake| {
        fake.remove_document(Visibility::Public, &rel("new.md"));
        let entity = Entity::Finding(finding(1, "body"));
        place(
            fake,
            Visibility::Public,
            "new.md",
            raw_document(entity, "identified\n"),
        );
    });
    let run = file_import(
        &world,
        Some(AdoptAfter {
            gate: Gate::Cr,
            source: SOURCE.to_owned(),
        }),
    );
    assert_eq!(refusal_of(run), Refusal::ConfirmDeclined);
    // The file was rewritten, and that stays; the adoption wrote nothing.
    assert_eq!(world.fake.writes(), 1);
    assert_eq!(world.records(Visibility::Public), 0);
}

#[test]
fn an_import_of_a_file_that_is_not_a_candidate_or_changed_is_refused() {
    let world = World::new();
    world.unidentified(Visibility::Public, "new.md", NEW);
    // No such file among the candidates.
    assert_eq!(
        refusal_of(import_run(
            &world,
            Import::File {
                at: at(Visibility::Public, "nowhere.md"),
                adopt: None
            }
        )),
        Refusal::ChangedUnderfoot(rel("nowhere.md"))
    );
    // The file went after the listing.
    world.fake.lose_file(Visibility::Public, &rel("new.md"));
    assert_eq!(
        refusal_of(file_import(&world, None)),
        Refusal::ChangedUnderfoot(rel("new.md"))
    );
    // Another editor wrote the file between the comparison and the replacement.
    let world = World::new();
    world.unidentified(Visibility::Public, "new.md", NEW);
    world
        .fake
        .race_on_next_write(Visibility::Public, &rel("new.md"), b"someone else\n");
    assert_eq!(
        refusal_of(file_import(&world, None)),
        Refusal::ChangedUnderfoot(rel("new.md"))
    );
    assert_eq!(
        world.fake.file(Visibility::Public, &rel("new.md")),
        Some(b"someone else\n".to_vec())
    );
}

#[test]
fn a_file_the_writer_cannot_keep_as_it_is_is_a_refusal_that_says_so() {
    let at_new = rel("new.md");
    type Prepare = fn(&World, &RelPath);
    type Refuses = fn(RelPath) -> Refusal;
    let cases: [(Prepare, Refuses); 4] = [
        (
            |world, at| world.fake.set_links(Visibility::Public, at, 2),
            Refusal::MultipleLinks,
        ),
        (
            |world, at| world.fake.make_irregular(Visibility::Public, at),
            Refusal::NotRegularFile,
        ),
        (
            |world, at| world.fake.make_group_unkeepable(Visibility::Public, at),
            Refusal::CannotKeepGroup,
        ),
        (
            |world, at| world.fake.make_owner_unkeepable(Visibility::Public, at),
            Refusal::CannotKeepOwner,
        ),
    ];
    for (prepare, refusal) in cases {
        let world = World::new();
        world.unidentified(Visibility::Public, "new.md", NEW);
        prepare(&world, &at_new);
        assert_eq!(
            refusal_of(file_import(&world, None)),
            refusal(at_new.clone())
        );
    }
}

#[test]
fn a_port_that_fails_while_importing_is_an_error() {
    let world = World::new();
    world.unidentified(Visibility::Public, "new.md", NEW);
    world.fake.fail(PortName::Files);
    let run = file_import(&world, None);
    assert_eq!(exit_class(&run), ExitClass::Port);
}

// --- import: the items of a Design ------------------------------------------------

const ITEMS: &str = "---\nspec: spec/s\nitems:\n  - number: 1\n    summary: the item\n    pins: [PIN-A]\n  - number: 2\n    summary: two\n    pins: []\n---\nnotes\n";

fn unidentified_design(world: &World, text: &str) {
    let entity = Entity::Design(design(
        DESIGN,
        SPEC,
        vec![(1, item("the item", &["PIN-A"])), (2, item("two", &[]))],
    ));
    world.add_text(Visibility::Public, "design.md", entity, text);
}

fn items_import(world: &World) -> Run<Imported> {
    import_run(
        world,
        Import::Items {
            design: docid(DESIGN),
        },
    )
}

#[test]
fn the_items_of_a_design_are_given_their_ids_and_the_rest_of_the_file_is_untouched() {
    let world = World::new();
    unidentified_design(&world, ITEMS);
    let imported = done(items_import(&world));
    assert_eq!(imported.doc(), docid(DESIGN));
    let [first, second] = items_of(&imported) else {
        panic!("two items were given an identity");
    };
    assert_eq!(
        world.fake.file(Visibility::Public, &rel("design.md")),
        Some(
            format!(
                "---\nspec: spec/s\nitems:\n  - number: 1\n    id: {first}\n    summary: the item\n    pins: [PIN-A]\n  - number: 2\n    id: {second}\n    summary: two\n    pins: []\n---\nnotes\n"
            )
            .into_bytes()
        )
    );
    assert!(world.fake.asked().is_empty());
}

#[test]
fn a_design_whose_items_have_their_ids_has_nothing_to_import() {
    let world = World::project(Visibility::Public);
    let imported = done(items_import(&world));
    assert!(items_of(&imported).is_empty());
    assert_eq!(world.fake.writes(), 0);
}

#[test]
fn a_design_whose_items_the_file_does_not_write_as_a_block_is_left_as_it_is() {
    let world = World::new();
    unidentified_design(
        &world,
        "---\nspec: spec/s\nitems:\n  - {number: 1, summary: x}\n  - {number: 2, summary: y}\n---\n",
    );
    assert_eq!(
        refusal_of(items_import(&world)),
        Refusal::CannotPlaceId(rel("design.md"))
    );
    assert_eq!(world.fake.writes(), 0);
}

#[test]
fn a_design_whose_publication_is_cut_short_is_not_rewritten_by_an_import() {
    let world = World::new();
    unidentified_design(&world, ITEMS);
    let entity = Entity::Design(design(
        DESIGN,
        SPEC,
        vec![(1, item("the item", &["PIN-A"])), (2, item("two", &[]))],
    ));
    world.add_text(Visibility::Private, "private.md", entity, ITEMS);
    assert_eq!(
        refusal_of(items_import(&world)),
        Refusal::PublishInProgress(docid(DESIGN))
    );
    assert_eq!(world.fake.writes(), 0);
    assert_eq!(
        world.fake.file(Visibility::Public, &rel("design.md")),
        Some(ITEMS.as_bytes().to_vec())
    );
}

#[test]
fn only_the_items_that_got_an_id_in_the_file_are_reported() {
    let world = World::new();
    unidentified_design(
        &world,
        "---\nspec: spec/s\nitems:\n  - number: 1\n    summary: the item\n    pins: [PIN-A]\n  - {number: 2, summary: two}\n---\n",
    );
    let imported = done(items_import(&world));
    let [first] = items_of(&imported) else {
        panic!("only item 1 got an identity: {imported:?}");
    };
    let written = world
        .fake
        .file(Visibility::Public, &rel("design.md"))
        .expect("the file");
    let written = String::from_utf8(written).expect("text");
    assert!(written.contains(&format!("    id: {first}\n")));
    assert_eq!(written.matches("id:").count(), 1);
}

#[test]
fn only_a_design_that_is_there_is_imported() {
    let world = World::project(Visibility::Public);
    assert_eq!(
        refusal_of(import_run(
            &world,
            Import::Items {
                design: docid(FINDING)
            }
        )),
        Refusal::Adoption(AdoptionRefusal::TargetInvalid {
            target: Target::Doc(docid(FINDING))
        })
    );
    assert_eq!(
        refusal_of(import_run(&world, Import::Items { design: docid(999) })),
        Refusal::Adoption(AdoptionRefusal::TargetUnknown {
            target: Target::Doc(docid(999))
        })
    );
}

#[test]
fn nobody_is_asked_who_is_acting_to_import() {
    // Importing asks the ports for an identity and for nothing about who acts.
    let world = World::new();
    world.unidentified(Visibility::Public, "new.md", NEW);
    world.fake.fail(PortName::Actor);
    assert!(file_import(&world, None).is_ok());
}
