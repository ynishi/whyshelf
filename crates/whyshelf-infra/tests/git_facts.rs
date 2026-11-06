//! The facts of the corpus model, against a real repository: where a path stands in
//! Git, who wrote a revision, the fingerprint of a change, and whether it landed.

mod common;

use std::path::Path;

use common::{commit_all, git, init_repo, write};
use whyshelf_core::app::ports::{BaselineQuery, RevisionFactsPort, WorkTree};
use whyshelf_core::domain::approval::CaChange;
use whyshelf_core::domain::corpus::{GitPlace, Visibility};
use whyshelf_core::domain::integration::{
    ApprovedChange, ChangeFingerprint, FingerprintUnavailable, Fingerprinting, HunkScheme,
    Integration, IntegrationEvidence, UnknownWhy,
};
use whyshelf_core::domain::knowable::Knowable;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::revision::{Revision, RevisionId};
use whyshelf_infra::git::GitFacts;

/// What a test asks of the answer when it expects a fingerprint.
trait Fingerprinted {
    fn expect(self, message: &str) -> ChangeFingerprint;
    fn unwrap(self) -> ChangeFingerprint;
    fn as_ref(&self) -> Option<&ChangeFingerprint>;
}

impl Fingerprinted for Fingerprinting {
    fn expect(self, message: &str) -> ChangeFingerprint {
        match self {
            Self::Of(fingerprint) => fingerprint,
            other => panic!("{message}: {other:?}"),
        }
    }

    fn unwrap(self) -> ChangeFingerprint {
        self.expect("a fingerprint")
    }

    fn as_ref(&self) -> Option<&ChangeFingerprint> {
        match self {
            Self::Of(fingerprint) => Some(fingerprint),
            Self::NothingToFingerprint | Self::Unavailable(_) => None,
        }
    }
}

/// A file of distinct, long lines, so that a change to it is a strong hunk.
fn lines(prefix: &str, count: usize) -> String {
    (1..=count)
        .map(|n| format!("let {prefix}_value_{n} = compute_something({n});\n"))
        .collect()
}

/// A repository on `main` with one commit.
async fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path()).await;
    write(dir.path(), "src/lib.rs", &lines("lib", 12)).await;
    write(dir.path(), "short.txt", "ok\n").await;
    commit_all(dir.path(), "Start").await;
    dir
}

fn commit(sha: &str) -> Revision {
    Revision::Commit(RevisionId::from_validated(sha.to_owned()).unwrap())
}

fn facts(dir: &Path) -> GitFacts {
    GitFacts::new(dir.to_path_buf(), "main".to_owned())
}

async fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"]).await
}

/// A feature branch off `main` with two commits that change `src/lib.rs`; returns
/// the revision of its tip. The working tree is left on the feature branch.
async fn feature(dir: &Path) -> String {
    git(dir, &["checkout", "-q", "-b", "feature"]).await;
    write(
        dir,
        "src/lib.rs",
        &lines("lib", 12).replace("lib_value_3", "lib_edited_3"),
    )
    .await;
    commit_all(dir, "Feature one").await;
    write(
        dir,
        "src/lib.rs",
        &(lines("lib", 12).replace("lib_value_3", "lib_edited_3")
            + "let appended_line_here = 1;\n"),
    )
    .await;
    commit_all(dir, "Feature two").await
}

/// Squash the feature branch into `main`.
async fn squash(dir: &Path) -> String {
    git(dir, &["checkout", "-q", "main"]).await;
    git(dir, &["merge", "-q", "--squash", "feature"]).await;
    commit_all(dir, "Squashed feature").await
}

async fn landed(
    port: &GitFacts,
    approved: &str,
    fingerprint: Option<&ChangeFingerprint>,
) -> (Knowable<bool>, Knowable<bool>, UnknownWhy) {
    let approved = commit(approved);
    let answer = port
        .baseline_facts(&BaselineQuery::new(&approved, fingerprint))
        .await
        .unwrap();
    (
        answer.facts().ancestor(),
        answer.facts().hunks_contained(),
        answer.cannot_tell(),
    )
}

// --- the fingerprint ------------------------------------------------------------------

#[tokio::test]
async fn a_change_of_two_commits_has_the_fingerprint_of_its_whole_diff() {
    let dir = repo().await;
    let branch_point = head(dir.path()).await;
    let approved = feature(dir.path()).await;
    let port = facts(dir.path());

    let fingerprint = port
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .expect("a change has a fingerprint");
    assert_eq!(fingerprint.base(), &commit(&branch_point));
    assert_eq!(
        fingerprint.baseline_at_approval(),
        &commit(&branch_point),
        "main had not moved"
    );
    assert_eq!(fingerprint.scheme(), &HunkScheme::V1);
    assert!(fingerprint.is_strong());
    // The edited line, and the appended one: two hunks cut with no context.
    assert_eq!(fingerprint.hunks().len(), 2);

    // The same change asked for again is the same fingerprint (and the first call is
    // not a state the second depends on).
    let again = port.change_fingerprint(&commit(&approved)).await.unwrap();
    assert_eq!(again.as_ref(), Some(&fingerprint));
}

#[tokio::test]
async fn the_base_is_where_the_change_branched_off_the_baseline_even_when_it_moved_on() {
    let dir = repo().await;
    let branch_point = head(dir.path()).await;
    let approved = feature(dir.path()).await;
    git(dir.path(), &["checkout", "-q", "main"]).await;
    write(dir.path(), "other.txt", &lines("other", 3)).await;
    let moved = commit_all(dir.path(), "Main moves on").await;

    let fingerprint = facts(dir.path())
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fingerprint.base(), &commit(&branch_point));
    assert_eq!(fingerprint.baseline_at_approval(), &commit(&moved));
    assert_eq!(
        fingerprint.hunks().len(),
        2,
        "the work of main is not in the change"
    );
}

#[tokio::test]
async fn a_change_that_branched_off_another_feature_branch_has_a_base_on_the_baseline() {
    let dir = repo().await;
    let branch_point = head(dir.path()).await;
    git(dir.path(), &["checkout", "-q", "-b", "feature-one"]).await;
    write(dir.path(), "one.txt", &lines("one", 4)).await;
    commit_all(dir.path(), "First of the stack").await;
    git(dir.path(), &["checkout", "-q", "-b", "feature-two"]).await;
    write(dir.path(), "two.txt", &lines("two", 4)).await;
    let approved = commit_all(dir.path(), "Second of the stack").await;

    let fingerprint = facts(dir.path())
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fingerprint.base(),
        &commit(&branch_point),
        "the line of the baseline, not the branch it was cut from"
    );
    assert_eq!(fingerprint.hunks().len(), 2, "both files are in the change");
}

#[tokio::test]
async fn nothing_is_fingerprinted_for_a_revision_on_the_baseline_or_a_patch_or_a_strange_one() {
    let dir = repo().await;
    let on_main = head(dir.path()).await;
    let port = facts(dir.path());
    assert_eq!(
        port.change_fingerprint(&commit(&on_main)).await.unwrap(),
        Fingerprinting::NothingToFingerprint,
        "a revision on the baseline has no change of its own"
    );
    let patch = Revision::Patch {
        base: RevisionId::from_validated(on_main.clone()).unwrap(),
        patch: whyshelf_core::domain::hash::ContentHash::raw(b"diff"),
    };
    assert_eq!(
        port.change_fingerprint(&patch).await.unwrap(),
        Fingerprinting::Unavailable(FingerprintUnavailable::NotCommitted),
        "a patch is not a commit"
    );
    // A commit this clone does not have: not shallow here, so the base is missing.
    assert_eq!(
        port.change_fingerprint(&commit(&"1".repeat(40)))
            .await
            .unwrap(),
        Fingerprinting::Unavailable(FingerprintUnavailable::BaseMissing)
    );
    // A baseline that is no commit.
    let nowhere = GitFacts::new(dir.path().to_path_buf(), "no-such-ref".to_owned());
    assert_eq!(
        nowhere.change_fingerprint(&commit(&on_main)).await.unwrap(),
        Fingerprinting::Unavailable(FingerprintUnavailable::BaselineUnresolved)
    );
}

#[tokio::test]
async fn a_shallow_clone_says_so_when_it_cannot_cut_the_change() {
    let origin = repo().await;
    let approved = feature(origin.path()).await;
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path().parent().unwrap(),
        &[
            "clone",
            "-q",
            "--depth=1",
            "--no-single-branch",
            &format!("file://{}", origin.path().display()),
            clone.path().to_str().unwrap(),
        ],
    )
    .await;
    // The approved commit is the tip of a branch the clone has; the commits before it
    // are cut off, so the point it branched off the baseline is not here.
    git(
        clone.path(),
        &["checkout", "-q", "-B", "main", "origin/main"],
    )
    .await;
    let got = GitFacts::new(clone.path().to_path_buf(), "main".to_owned())
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap();
    assert_eq!(
        got,
        Fingerprinting::Unavailable(FingerprintUnavailable::Shallow)
    );
}

// --- the work tree ----------------------------------------------------------------------

#[tokio::test]
async fn the_work_tree_is_clean_dirty_or_not_one() {
    let dir = repo().await;
    let port = facts(dir.path());
    assert_eq!(
        port.work_tree().await.unwrap(),
        Knowable::Known(WorkTree::Clean)
    );
    write(dir.path(), "src/lib.rs", "changed\n").await;
    assert_eq!(
        port.work_tree().await.unwrap(),
        Knowable::Known(WorkTree::Dirty)
    );
    // A directory that is no repository cannot say: not dirty, and not clean.
    let nowhere = tempfile::tempdir().unwrap();
    assert_eq!(
        facts(nowhere.path()).work_tree().await.unwrap(),
        Knowable::Unknown
    );
}

// --- whether it landed ------------------------------------------------------------------

#[tokio::test]
async fn a_squash_merge_lands_by_its_hunks_and_not_by_ancestry() {
    let dir = repo().await;
    let approved = feature(dir.path()).await;
    let port = facts(dir.path());
    let fingerprint = port
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .unwrap();

    // Before the merge: nothing landed, and that is known.
    let (ancestor, hunks, _) = landed(&port, &approved, Some(&fingerprint)).await;
    assert_eq!(
        (ancestor, hunks),
        (Knowable::Known(false), Knowable::Known(false))
    );

    squash(dir.path()).await;
    let (ancestor, hunks, why) = landed(&port, &approved, Some(&fingerprint)).await;
    assert_eq!(
        ancestor,
        Knowable::Known(false),
        "the commit is not in main"
    );
    assert_eq!(hunks, Knowable::Known(true), "its hunks are");

    let answer = port
        .baseline_facts(&BaselineQuery::new(&commit(&approved), Some(&fingerprint)))
        .await
        .unwrap();
    let verdict = Integration::decide([ApprovedChange::checked(
        commit(&approved),
        CaChange::Fingerprinted(fingerprint),
        answer.facts(),
        why,
    )]);
    assert_eq!(
        verdict,
        Integration::Integrated {
            evidence: IntegrationEvidence::HunksContained
        }
    );
}

#[tokio::test]
async fn a_merge_commit_lands_by_ancestry() {
    let dir = repo().await;
    let approved = feature(dir.path()).await;
    let port = facts(dir.path());
    let fingerprint = port
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .unwrap();
    git(dir.path(), &["checkout", "-q", "main"]).await;
    git(
        dir.path(),
        &["merge", "-q", "--no-ff", "-m", "Merge", "feature"],
    )
    .await;

    let (ancestor, hunks, _) = landed(&port, &approved, Some(&fingerprint)).await;
    assert_eq!(ancestor, Knowable::Known(true));
    assert_eq!(
        hunks,
        Knowable::Known(true),
        "the merge commit, against its first parent, brought the same hunks"
    );
}

#[tokio::test]
async fn a_change_of_weak_hunks_alone_needs_the_ancestry() {
    let dir = repo().await;
    git(dir.path(), &["checkout", "-q", "-b", "feature"]).await;
    write(dir.path(), "short.txt", "no\n").await;
    let approved = commit_all(dir.path(), "A short edit").await;
    let port = facts(dir.path());
    let fingerprint = port
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .unwrap();
    assert!(!fingerprint.is_strong(), "`ok` to `no` is a weak hunk");

    squash(dir.path()).await;
    let answer = port
        .baseline_facts(&BaselineQuery::new(&commit(&approved), Some(&fingerprint)))
        .await
        .unwrap();
    // The port tells the truth about the hunks; the rule that a weak match counts for
    // nothing is the domain's.
    assert_eq!(answer.facts().hunks_contained(), Knowable::Known(true));
    let verdict = Integration::decide([ApprovedChange::checked(
        commit(&approved),
        CaChange::Fingerprinted(fingerprint.clone()),
        answer.facts(),
        answer.cannot_tell(),
    )]);
    assert_eq!(
        verdict,
        Integration::Pending {
            approved: commit(&approved)
        },
        "squashed, a weak change has not landed as far as anyone can tell"
    );

    // Merged with a merge commit it is an ancestor, and that is enough.
    git(dir.path(), &["reset", "-q", "--hard", "HEAD~1"]).await;
    git(
        dir.path(),
        &["merge", "-q", "--no-ff", "-m", "Merge", "feature"],
    )
    .await;
    let answer = port
        .baseline_facts(&BaselineQuery::new(&commit(&approved), Some(&fingerprint)))
        .await
        .unwrap();
    let verdict = Integration::decide([ApprovedChange::checked(
        commit(&approved),
        CaChange::Fingerprinted(fingerprint),
        answer.facts(),
        answer.cannot_tell(),
    )]);
    assert!(matches!(
        verdict,
        Integration::Integrated {
            evidence: IntegrationEvidence::Ancestor(_)
        }
    ));
}

#[tokio::test]
async fn an_approval_made_after_the_squash_finds_it_before_the_tip_at_approval() {
    let dir = repo().await;
    let approved = feature(dir.path()).await;
    squash(dir.path()).await;
    let port = facts(dir.path());
    let fingerprint = port
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .unwrap();

    let (_, hunks, _) = landed(&port, &approved, Some(&fingerprint)).await;
    assert_eq!(
        hunks,
        Knowable::Known(true),
        "the commit between the base and the tip at approval is looked through"
    );

    // Too many commits to look through there, and the change not found in the part
    // looked at: that is not a no.
    let small = facts(dir.path()).with_limits(1000, 0);
    let (_, hunks, why) = landed(&small, &approved, Some(&fingerprint)).await;
    assert_eq!(hunks, Knowable::Unknown);
    assert_eq!(why, UnknownWhy::OutsideWindow);
}

#[tokio::test]
async fn a_window_that_ends_before_the_change_is_found_is_unknown_not_no() {
    let dir = repo().await;
    let approved = feature(dir.path()).await;
    git(dir.path(), &["checkout", "-q", "main"]).await;
    let port = facts(dir.path());
    let fingerprint = port
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .unwrap();
    // Main moves on, then the squash lands.
    write(dir.path(), "other.txt", &lines("other", 3)).await;
    commit_all(dir.path(), "Unrelated work").await;
    git(dir.path(), &["merge", "-q", "--squash", "feature"]).await;
    commit_all(dir.path(), "Squashed feature").await;

    let (_, hunks, _) = landed(&port, &approved, Some(&fingerprint)).await;
    assert_eq!(hunks, Knowable::Known(true));

    let short = facts(dir.path()).with_limits(1, 5000);
    let (ancestor, hunks, why) = landed(&short, &approved, Some(&fingerprint)).await;
    assert_eq!(ancestor, Knowable::Known(false));
    assert_eq!(hunks, Knowable::Unknown);
    assert_eq!(why, UnknownWhy::OutsideWindow);
    // With room for both commits it is found again.
    let enough = facts(dir.path()).with_limits(2, 5000);
    let (_, hunks, _) = landed(&enough, &approved, Some(&fingerprint)).await;
    assert_eq!(hunks, Knowable::Known(true));

    // A repository that has neither the commit nor where it branched off cannot say
    // no either.
    let other = tempfile::tempdir().unwrap();
    init_repo(other.path()).await;
    write(other.path(), "a.txt", &lines("a", 3)).await;
    commit_all(other.path(), "Start").await;
    let (ancestor, hunks, _) = landed(&facts(other.path()), &approved, Some(&fingerprint)).await;
    assert_eq!(
        (ancestor, hunks),
        (Knowable::Unknown, Knowable::Unknown),
        "another repository has neither the commit nor the base"
    );
}

#[tokio::test]
async fn a_shallow_clone_cannot_say_and_says_why() {
    let dir = repo().await;
    let approved = feature(dir.path()).await;
    let port = facts(dir.path());
    let fingerprint = port
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .unwrap();
    squash(dir.path()).await;
    write(dir.path(), "later.txt", &lines("later", 3)).await;
    commit_all(dir.path(), "Later work").await;

    let clones = tempfile::tempdir().unwrap();
    let shallow = clones.path().join("shallow");
    git(
        clones.path(),
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            "--no-local",
            &format!("file://{}", dir.path().display()),
            shallow.to_str().unwrap(),
        ],
    )
    .await;
    let port = facts(&shallow);
    let (ancestor, hunks, why) = landed(&port, &approved, Some(&fingerprint)).await;
    assert_eq!(
        ancestor,
        Knowable::Unknown,
        "the approved commit is not here"
    );
    assert_eq!(hunks, Knowable::Unknown, "nor is where it branched off");
    assert_eq!(why, UnknownWhy::Shallow);
}

#[tokio::test]
async fn a_base_that_is_not_on_the_line_in_a_clone_that_is_not_shallow_is_base_missing() {
    let dir = repo().await;
    let tip = head(dir.path()).await;
    let invented = ChangeFingerprint::of_hunks(
        commit(&"1".repeat(40)),
        commit(&tip),
        &[whyshelf_core::domain::integration::Hunk::new(
            "src/lib.rs".parse::<RelPath>().unwrap(),
            vec![],
            vec!["a line that is long enough to count".to_owned()],
        )],
        HunkScheme::V1,
    )
    .unwrap();
    let (_, hunks, why) = landed(&facts(dir.path()), &tip, Some(&invented)).await;
    assert_eq!(hunks, Knowable::Unknown);
    assert_eq!(why, UnknownWhy::BaseMissing);
}

#[tokio::test]
async fn what_cannot_be_matched_is_unknown_for_its_own_reason() {
    let dir = repo().await;
    let approved = feature(dir.path()).await;
    let port = facts(dir.path());
    let fingerprint = port
        .change_fingerprint(&commit(&approved))
        .await
        .unwrap()
        .unwrap();

    // No fingerprint.
    let (_, hunks, why) = landed(&port, &approved, None).await;
    assert_eq!((hunks, why), (Knowable::Unknown, UnknownWhy::NoFingerprint));

    // A baseline that is no commit says that first.
    let nowhere = GitFacts::new(dir.path().to_path_buf(), "no-such-ref".to_owned());
    let (ancestor, hunks, why) = landed(&nowhere, &approved, Some(&fingerprint)).await;
    assert_eq!(
        (ancestor, hunks, why),
        (
            Knowable::Unknown,
            Knowable::Unknown,
            UnknownWhy::BaselineUnresolved
        )
    );

    // A fingerprint of a scheme this build does not have.
    let future = ChangeFingerprint::new(
        fingerprint.base().clone(),
        fingerprint.baseline_at_approval().clone(),
        fingerprint.hunks().clone(),
        true,
        HunkScheme::from_name("hunks-v99").unwrap(),
    );
    let (_, hunks, why) = landed(&port, &approved, Some(&future)).await;
    assert_eq!(
        (hunks, why),
        (Knowable::Unknown, UnknownWhy::FingerprintVersion)
    );
}

#[tokio::test]
async fn a_patch_is_never_an_ancestor() {
    let dir = repo().await;
    let tip = head(dir.path()).await;
    let patch = Revision::Patch {
        base: RevisionId::from_validated(tip).unwrap(),
        patch: whyshelf_core::domain::hash::ContentHash::raw(b"diff"),
    };
    let answer = facts(dir.path())
        .baseline_facts(&BaselineQuery::new(&patch, None))
        .await
        .unwrap();
    assert_eq!(answer.facts().ancestor(), Knowable::Known(false));
}

// --- who wrote it, and where a path stands -------------------------------------------------

#[tokio::test]
async fn the_author_is_the_name_and_the_mail_of_the_commit() {
    let dir = repo().await;
    let tip = head(dir.path()).await;
    let port = facts(dir.path());
    let Knowable::Known(author) = port.author(&commit(&tip)).await.unwrap() else {
        panic!("the commit is here");
    };
    assert_eq!(author.name().as_str(), "Test Author");
    assert_eq!(author.email().unwrap().as_str(), "author@example.invalid");

    assert_eq!(
        port.author(&commit(&"1".repeat(40))).await.unwrap(),
        Knowable::Unknown,
        "a commit this clone does not have"
    );
    let patch = Revision::Patch {
        base: RevisionId::from_validated(tip).unwrap(),
        patch: whyshelf_core::domain::hash::ContentHash::raw(b"diff"),
    };
    assert_eq!(port.author(&patch).await.unwrap(), Knowable::Unknown);
}

fn path(text: &str) -> RelPath {
    text.parse().unwrap()
}

#[tokio::test]
async fn git_says_tracked_ignored_untracked_or_outside() {
    let dir = repo().await;
    write(dir.path(), ".gitignore", "private/\n").await;
    write(dir.path(), "docs/a.md", "tracked\n").await;
    commit_all(dir.path(), "Docs").await;
    write(dir.path(), "docs/new.md", "not added\n").await;
    write(dir.path(), "private/p.md", "hidden\n").await;

    let outside = tempfile::tempdir().unwrap();
    let port = facts(dir.path()).with_private_root(outside.path().to_path_buf());
    let place = |root, text: &'static str| {
        let port = port.clone();
        async move { port.git_place(root, &path(text)).await.unwrap() }
    };
    assert_eq!(
        place(Visibility::Public, "docs/a.md").await,
        Knowable::Known(GitPlace::Tracked)
    );
    assert_eq!(
        place(Visibility::Public, "docs/new.md").await,
        Knowable::Known(GitPlace::Untracked)
    );
    assert_eq!(
        place(Visibility::Public, "private/p.md").await,
        Knowable::Known(GitPlace::Ignored)
    );
    assert_eq!(
        place(Visibility::Public, "docs/not-yet.md").await,
        Knowable::Known(GitPlace::Untracked),
        "a file that is not there yet can still be asked about"
    );
    assert_eq!(
        place(Visibility::Private, "anything.md").await,
        Knowable::Known(GitPlace::Outside),
        "a private root outside the work tree"
    );

    // A private root inside the work tree, in a place the repository ignores, and one
    // in a place it does not.
    let inside = facts(dir.path()).with_private_root(dir.path().join("private"));
    assert_eq!(
        inside
            .git_place(Visibility::Private, &path("p.md"))
            .await
            .unwrap(),
        Knowable::Known(GitPlace::Ignored)
    );
    let exposed = facts(dir.path()).with_private_root(dir.path().join("docs"));
    assert_eq!(
        exposed
            .git_place(Visibility::Private, &path("a.md"))
            .await
            .unwrap(),
        Knowable::Known(GitPlace::Tracked)
    );
    assert_eq!(
        exposed
            .git_place(Visibility::Private, &path("new.md"))
            .await
            .unwrap(),
        Knowable::Known(GitPlace::Untracked)
    );

    // No private root known, or one that is not there: Git cannot say.
    let unset = facts(dir.path());
    assert_eq!(
        unset
            .git_place(Visibility::Private, &path("x.md"))
            .await
            .unwrap(),
        Knowable::Unknown
    );
    let gone = facts(dir.path()).with_private_root(dir.path().join("nowhere"));
    assert_eq!(
        gone.git_place(Visibility::Private, &path("x.md"))
            .await
            .unwrap(),
        Knowable::Unknown
    );
}

#[tokio::test]
async fn outside_a_repository_git_cannot_say() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.md", "x\n").await;
    let port = facts(dir.path());
    assert_eq!(
        port.git_place(Visibility::Public, &path("a.md"))
            .await
            .unwrap(),
        Knowable::Unknown
    );
}
