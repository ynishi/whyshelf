//! The fake of the corpus ports does what the port documents say.

use super::{FakeCorpus, FakePorts, Reply, block_on};
use crate::app::commit::fixtures::{
    ada, approval_of, no_ledgers, object_view, plain_view, private_corpus, public_corpus, text_of,
};
use crate::app::commit::record::{append_with_object, scan, store};
use crate::app::outcome::Refusal;
use crate::app::ports::{
    ActorSource, Author, BaselineAnswer, BaselineQuery, ConfirmPort, ConfirmPrompt, Confirmed,
    CorpusPorts, CorruptRecord, DocumentSource, FileWriter, KeySource, LedgerStore, ObjectStore,
    PlanSummary, PortName, Put, RecordProblem, RevisionFactsPort, Write,
};
use crate::app::raw::RawDocument;
use crate::domain::corpus::{FileFacts, GitPlace, Location, Reading, Visibility};
use crate::domain::hash::{BytesHash, TextHash};
use crate::domain::id::test_support::docid;
use crate::domain::integration::{BaselineFacts, ChangeFingerprint, Hunk, HunkScheme, UnknownWhy};
use crate::domain::judge::test_support::{agent, judge};
use crate::domain::judge::{Email, PersonName};
use crate::domain::judgement::test_support::{cr_passed, jid, object_of, ts, why};
use crate::domain::judgement::{Judgement, JudgementKind};
use crate::domain::keyring::PinKeyring;
use crate::domain::knowable::Knowable;
use crate::domain::location::RelPath;
use crate::domain::revision::{Revision, RevisionId};
use crate::domain::stored::StoredJudgement;

fn rel(path: &str) -> RelPath {
    path.parse().expect("a path")
}

fn at(root: Visibility, path: &str) -> Location {
    Location::new(root, rel(path), Knowable::Unknown)
}

fn prompt() -> ConfirmPrompt {
    let plan = crate::app::commit::fixtures::Publishing::of(b"bytes").plan;
    ConfirmPrompt::new(
        plan.digest(),
        PlanSummary::new(crate::app::ports::ConfirmAction::Publish, 2),
    )
}

/// An approval of the Finding at `tag` written to the public ledger and its object
/// stored, through the commit steps.
fn write_approval(fake: &FakeCorpus, judgement: &Judgement, tag: &str) {
    let judgement = object_view(judgement.clone());
    block_on(async {
        let text = text_of(tag);
        let scanned = scan(
            &FakePorts::new(),
            &public_corpus(),
            &no_ledgers(),
            &text,
            &judgement,
        )
        .await
        .expect("scanned");
        let stored = store(fake, &scanned).await.expect("stored");
        append_with_object(fake, judgement, &stored)
            .await
            .expect("appended");
    });
}

// --- the ledgers ------------------------------------------------------------------

#[test]
fn a_ledger_reads_back_what_was_appended_and_nothing_of_the_other_side() {
    let fake = FakeCorpus::new();
    let judgement = approval_of("v1", Visibility::Public);
    write_approval(&fake, &judgement, "v1");

    let public = block_on(fake.read(Visibility::Public)).expect("read");
    assert_eq!(public.records(), [StoredJudgement::from(judgement)]);
    assert!(public.corrupt().is_empty());
    let private = block_on(fake.read(Visibility::Private)).expect("read");
    assert!(private.records().is_empty());
}

#[test]
fn appending_the_same_record_again_is_success_and_changes_nothing() {
    let fake = FakeCorpus::new();
    let judgement = approval_of("v1", Visibility::Public);
    write_approval(&fake, &judgement, "v1");
    write_approval(&fake, &judgement, "v1");
    assert_eq!(fake.records(Visibility::Public).len(), 1);
}

#[test]
fn a_record_of_the_same_id_with_other_bytes_is_kept_beside_the_first_and_reads_as_corrupt() {
    let fake = FakeCorpus::new();
    let first = approval_of("v1", Visibility::Public);
    let second = Judgement::approve(
        first.id(),
        ts(),
        judge("bob"),
        Visibility::Public,
        cr_passed(1, "v1"),
        object_of(&crate::domain::judgement::test_support::hash_of("v1")),
    )
    .expect("an approval");
    assert_ne!(first.canonical_bytes(), second.canonical_bytes());
    write_approval(&fake, &first, "v1");
    write_approval(&fake, &second, "v1");

    let read = block_on(fake.read(Visibility::Public)).expect("read");
    assert_eq!(read.records().len(), 2);
    assert!(read.ledger().is_corrupt(&first.id()));
}

#[test]
fn records_that_could_not_be_decoded_are_listed_with_the_ledger() {
    let fake = FakeCorpus::new();
    let bad = CorruptRecord::new(jid(5), RecordProblem::Undecodable);
    fake.seed_corrupt(Visibility::Private, bad);
    let read = block_on(fake.read(Visibility::Private)).expect("read");
    assert_eq!(read.corrupt(), [bad]);
    assert!(read.records().is_empty());
}

// --- the objects are a 2P-set -----------------------------------------------------

#[test]
fn a_stored_text_is_read_back_from_its_side_and_a_purged_one_is_gone_and_stays_gone() {
    let fake = FakeCorpus::new();
    let judgement = approval_of("v1", Visibility::Public);
    let hash = *judgement.object().expect("an object").text();
    write_approval(&fake, &judgement, "v1");

    let text = text_of("v1");
    assert_eq!(
        block_on(fake.get(Visibility::Public, hash)).expect("get"),
        Some(text.as_str().to_owned())
    );
    assert_eq!(
        block_on(fake.get(Visibility::Private, hash)).expect("get"),
        None
    );

    fake.purge(Visibility::Public, hash);
    assert_eq!(
        block_on(fake.get(Visibility::Public, hash)).expect("get"),
        None
    );
    let scanned = block_on(scan(
        &FakePorts::new(),
        &public_corpus(),
        &no_ledgers(),
        &text,
        &object_view(judgement.clone()),
    ))
    .expect("scanned");
    assert_eq!(block_on(fake.put(&scanned)).expect("put"), Put::Purged);
    assert_eq!(
        block_on(fake.get(Visibility::Public, hash)).expect("get"),
        None
    );
}

#[test]
fn a_branch_that_stored_a_text_before_the_purge_reads_it_as_gone_after_the_merge() {
    let judgement = approval_of("v1", Visibility::Public);
    let hash = *judgement.object().expect("an object").text();

    let diverged = FakeCorpus::new();
    write_approval(&diverged, &judgement, "v1");
    assert!(diverged.has_object(Visibility::Public, hash));

    let purged = FakeCorpus::new();
    purged.purge(Visibility::Public, hash);

    diverged.merge_objects_from(&purged);
    assert!(!diverged.has_object(Visibility::Public, hash));
    assert_eq!(
        block_on(diverged.get(Visibility::Public, hash)).expect("get"),
        None
    );

    // The merge is a union either way round.
    purged.merge_objects_from(&diverged);
    assert!(!purged.has_object(Visibility::Public, hash));
}

#[test]
fn a_purge_record_in_the_ledger_takes_the_text_out_of_the_store() {
    let fake = FakeCorpus::new();
    let judgement = approval_of("v1", Visibility::Public);
    let hash = *judgement.object().expect("an object").text();
    write_approval(&fake, &judgement, "v1");

    let purge = Judgement::purge(
        jid(7),
        ts(),
        ada(),
        Visibility::Public,
        docid(1),
        hash,
        why(),
    );
    assert!(matches!(purge.kind(), JudgementKind::Purge { .. }));
    block_on(crate::app::commit::record::append_without_object(
        &fake,
        plain_view(purge),
        &public_corpus(),
        &no_ledgers(),
    ))
    .expect("appended");
    assert!(!fake.has_object(Visibility::Public, hash));
}

#[test]
fn a_purge_takes_the_text_out_of_its_own_sides_store_only() {
    let fake = FakeCorpus::new();
    let public = approval_of("v1", Visibility::Public);
    let hash = *public.object().expect("an object").text();
    write_approval(&fake, &public, "v1");

    // A purge in the private ledger of the same text: the public store keeps it.
    let purge = Judgement::purge(
        jid(8),
        ts(),
        ada(),
        Visibility::Private,
        docid(1),
        hash,
        why(),
    );
    block_on(crate::app::commit::record::append_without_object(
        &fake,
        plain_view(purge),
        &private_corpus(),
        &no_ledgers(),
    ))
    .expect("appended");
    assert!(fake.has_object(Visibility::Public, hash));
    assert_eq!(
        block_on(fake.get(Visibility::Public, hash)).expect("get"),
        Some(text_of("v1").as_str().to_owned())
    );
}

// --- who is acting, and the keys --------------------------------------------------

#[test]
fn the_actor_is_the_one_set_and_an_agent_stays_an_agent() {
    let fake = FakeCorpus::new();
    assert_eq!(block_on(fake.actor()).expect("actor"), ada());
    fake.set_actor(agent("ada", "some-tool"));
    assert_eq!(
        block_on(fake.actor()).expect("actor"),
        agent("ada", "some-tool")
    );
}

#[test]
fn a_person_without_a_key_has_an_empty_keyring_and_a_place_without_one_is_unknown() {
    let fake = FakeCorpus::new();
    assert!(matches!(
        block_on(fake.keys()).expect("keys"),
        Knowable::Known(_)
    ));
    fake.set_keys(Knowable::Unknown);
    assert!(block_on(fake.keys()).expect("keys").known().is_none());
    fake.set_keys(Knowable::Known(PinKeyring::empty()));
    assert!(block_on(fake.keys()).expect("keys").known().is_some());
}

// --- the person -------------------------------------------------------------------

#[test]
fn the_person_answers_yes_no_or_unavailable_and_is_asked_what_the_prompt_says() {
    let fake = FakeCorpus::new();
    let prompt = prompt();
    for answer in [Confirmed::Yes, Confirmed::No, Confirmed::Unavailable] {
        fake.set_reply(Reply::Always(answer));
        assert_eq!(block_on(fake.ask(&prompt)).expect("asked"), answer);
    }
    assert_eq!(fake.asked(), vec![prompt; 3]);

    fake.set_reply(Reply::TypesBack(prompt.plan()));
    assert_eq!(block_on(fake.ask(&prompt)).expect("asked"), Confirmed::Yes);
}

// --- the file writer --------------------------------------------------------------

#[test]
fn create_new_makes_the_file_and_does_not_replace_one() {
    let fake = FakeCorpus::new();
    let place = at(Visibility::Public, "docs/a.md");
    assert_eq!(
        block_on(fake.create_new(&place, b"one")).expect("write"),
        Write::Done
    );
    assert_eq!(
        block_on(fake.create_new(&place, b"two")).expect("write"),
        Write::Exists
    );
    assert_eq!(
        fake.file(Visibility::Public, &rel("docs/a.md")),
        Some(b"one".to_vec())
    );
    // The same path under the other root is another file.
    assert_eq!(
        block_on(fake.create_new(&at(Visibility::Private, "docs/a.md"), b"three")).expect("write"),
        Write::Done
    );
}

#[test]
fn rewrite_replaces_the_file_it_compared() {
    let fake = FakeCorpus::new();
    let path = rel("docs/a.md");
    fake.put_file(Visibility::Public, &path, b"one");
    let place = at(Visibility::Public, "docs/a.md");
    assert_eq!(
        block_on(fake.rewrite(&place, BytesHash::of(b"one"), b"two")).expect("write"),
        Write::Done
    );
    assert_eq!(fake.file(Visibility::Public, &path), Some(b"two".to_vec()));
}

#[test]
fn a_file_that_is_not_what_the_caller_saw_is_changed_underfoot_and_left_alone() {
    let fake = FakeCorpus::new();
    let path = rel("docs/a.md");
    fake.put_file(Visibility::Public, &path, b"edited since");
    let place = at(Visibility::Public, "docs/a.md");

    assert_eq!(
        block_on(fake.rewrite(&place, BytesHash::of(b"what was read"), b"new")).expect("write"),
        Write::ChangedUnderfoot
    );
    assert_eq!(
        block_on(fake.remove(&place, BytesHash::of(b"what was read"))).expect("write"),
        Write::ChangedUnderfoot
    );
    assert_eq!(
        fake.file(Visibility::Public, &path),
        Some(b"edited since".to_vec())
    );
    // A file that is not there is not the file that was read either.
    assert_eq!(
        block_on(fake.remove(&at(Visibility::Public, "docs/none.md"), BytesHash::of(b"x")))
            .expect("write"),
        Write::ChangedUnderfoot
    );
}

#[test]
fn another_editor_between_the_comparison_and_the_replacement_is_changed_underfoot() {
    let fake = FakeCorpus::new();
    let path = rel("docs/a.md");
    fake.put_file(Visibility::Public, &path, b"one");
    let place = at(Visibility::Public, "docs/a.md");

    // The first comparison passes; then the editor writes; the last comparison fails.
    fake.race_on_next_write(Visibility::Public, &path, b"the editor's");
    assert_eq!(
        block_on(fake.rewrite(&place, BytesHash::of(b"one"), b"two")).expect("write"),
        Write::ChangedUnderfoot
    );
    assert_eq!(
        fake.file(Visibility::Public, &path),
        Some(b"the editor's".to_vec())
    );

    fake.put_file(Visibility::Public, &path, b"one");
    fake.race_on_next_write(Visibility::Public, &path, b"the editor's");
    assert_eq!(
        block_on(fake.remove(&place, BytesHash::of(b"one"))).expect("write"),
        Write::ChangedUnderfoot
    );
    assert!(fake.file(Visibility::Public, &path).is_some());
}

#[test]
fn remove_takes_away_the_file_it_compared() {
    let fake = FakeCorpus::new();
    let path = rel("docs/a.md");
    fake.put_file(Visibility::Private, &path, b"one");
    assert_eq!(
        block_on(fake.remove(&at(Visibility::Private, "docs/a.md"), BytesHash::of(b"one")))
            .expect("write"),
        Write::Done
    );
    assert_eq!(fake.file(Visibility::Private, &path), None);
}

#[test]
fn a_file_with_links_a_link_or_a_group_it_cannot_keep_is_not_replaced() {
    let fake = FakeCorpus::new();
    let path = rel("docs/a.md");
    let place = at(Visibility::Public, "docs/a.md");
    let expect = BytesHash::of(b"one");

    fake.put_file(Visibility::Public, &path, b"one");
    fake.set_links(Visibility::Public, &path, 2);
    assert_eq!(
        block_on(fake.rewrite(&place, expect, b"two")).expect("write"),
        Write::MultipleLinks
    );

    fake.put_file(Visibility::Public, &path, b"one");
    fake.make_irregular(Visibility::Public, &path);
    assert_eq!(
        block_on(fake.rewrite(&place, expect, b"two")).expect("write"),
        Write::NotRegularFile
    );
    assert_eq!(
        block_on(fake.remove(&place, expect)).expect("write"),
        Write::NotRegularFile
    );

    fake.put_file(Visibility::Public, &path, b"one");
    fake.make_group_unkeepable(Visibility::Public, &path);
    assert_eq!(
        block_on(fake.rewrite(&place, expect, b"two")).expect("write"),
        Write::CannotKeepGroup
    );
    assert_eq!(fake.file(Visibility::Public, &path), Some(b"one".to_vec()));
}

#[test]
fn each_write_that_did_not_happen_has_a_refusal_with_the_path() {
    let path = rel("docs/a.md");
    assert_eq!(Write::Done.refusal(&path), None);
    assert_eq!(Write::Exists.refusal(&path), None);
    assert_eq!(
        Write::ChangedUnderfoot.refusal(&path),
        Some(Refusal::ChangedUnderfoot(path.clone()))
    );
    assert_eq!(
        Write::MultipleLinks.refusal(&path),
        Some(Refusal::MultipleLinks(path.clone()))
    );
    assert_eq!(
        Write::NotRegularFile.refusal(&path),
        Some(Refusal::NotRegularFile(path.clone()))
    );
    assert_eq!(
        Write::CannotKeepGroup.refusal(&path),
        Some(Refusal::CannotKeepGroup(path.clone()))
    );
}

// --- documents and version control ------------------------------------------------

#[test]
fn the_documents_are_listed_as_they_were_added() {
    let fake = FakeCorpus::new();
    assert!(block_on(fake.list()).expect("list").is_empty());
    let facts = FileFacts::new(at(Visibility::Public, "docs/a.md"), BytesHash::of(b"a"));
    let raw = RawDocument::new(
        Reading::Unidentified { kind: None },
        crate::app::raw::NormalizedText::new("a\n".to_owned()),
    );
    fake.add_document(facts.clone(), raw.clone());
    assert_eq!(block_on(fake.list()).expect("list"), vec![(facts, raw)]);
}

#[test]
fn version_control_answers_what_it_was_told_and_unknown_otherwise() {
    let fake = FakeCorpus::new();
    let commit = Revision::Commit(RevisionId::from_validated("a1".to_owned()).expect("a revision"));

    assert_eq!(
        block_on(fake.author(&commit)).expect("author"),
        Knowable::Unknown
    );
    let author = Author::new(
        PersonName::new("ada").expect("a name"),
        Some(Email::new("ada@example.org").expect("a mail")),
    );
    fake.set_author(commit.clone(), author.clone());
    assert_eq!(
        block_on(fake.author(&commit)).expect("author"),
        Knowable::Known(author.clone())
    );
    assert!(author.is_judge(&ada()));

    let path = rel("docs/a.md");
    assert_eq!(
        block_on(fake.git_place(Visibility::Private, &path)).expect("place"),
        Knowable::Unknown
    );
    fake.set_place(Visibility::Private, &path, GitPlace::Ignored);
    assert_eq!(
        block_on(fake.git_place(Visibility::Private, &path)).expect("place"),
        Knowable::Known(GitPlace::Ignored)
    );

    let answer = BaselineAnswer::new(
        BaselineFacts::new(Knowable::Known(true), Knowable::Unknown),
        UnknownWhy::BaseMissing,
    );
    fake.set_baseline(answer);
    assert_eq!(
        block_on(fake.baseline_facts(&BaselineQuery::new(&commit, None))).expect("facts"),
        answer
    );
    assert_eq!(
        block_on(fake.change_fingerprint(&commit)).expect("fingerprint"),
        None
    );
    let fingerprint = ChangeFingerprint::of_hunks(
        commit.clone(),
        commit.clone(),
        &[Hunk::new(
            rel("src/a.rs"),
            Vec::new(),
            vec!["let a = 1;".to_owned()],
        )],
        HunkScheme::CURRENT,
    )
    .expect("a fingerprint");
    fake.set_fingerprint(Some(fingerprint.clone()));
    assert_eq!(
        block_on(fake.change_fingerprint(&commit)).expect("fingerprint"),
        Some(fingerprint)
    );
}

#[test]
fn an_author_who_is_another_person_is_not_the_judge() {
    let other = Author::new(PersonName::new("bob").expect("a name"), None);
    assert!(!other.is_judge(&ada()));
    let same_name_other_mail = Author::new(
        PersonName::new("ada").expect("a name"),
        Some(Email::new("a@one.example").expect("a mail")),
    );
    let judge_with_mail = crate::domain::judge::Judge::new(
        PersonName::new("ada").expect("a name"),
        Some(Email::new("b@two.example").expect("a mail")),
        crate::domain::judge::Principal::Human,
    );
    assert!(!same_name_other_mail.is_judge(&judge_with_mail));
    assert!(same_name_other_mail.is_judge(&ada()));
}

#[test]
fn a_port_made_to_fail_fails_with_its_own_error_and_the_others_do_not() {
    let fake = FakeCorpus::new();
    fake.fail(PortName::Documents);
    assert!(block_on(fake.list()).is_err());
    assert!(block_on(fake.actor()).is_ok());
}

#[test]
fn the_fake_is_a_bundle_of_the_corpus_ports() {
    fn who<P: CorpusPorts>(ports: &P) -> TextHash {
        let _ = ports.documents();
        let _ = ports.ledger();
        let _ = ports.objects();
        let _ = ports.actor_source();
        let _ = ports.key_source();
        let _ = ports.confirm();
        let _ = ports.files();
        let _ = ports.facts();
        TextHash::of("a bundle")
    }
    assert_eq!(who(&FakeCorpus::new()), TextHash::of("a bundle"));
    assert!(docid(1) == docid(1));
}
