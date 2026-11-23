//! Tests of the walk along the links of a name and of the case of a name as written,
//! over a tree held in memory (the adapter's answers as given) and over the file
//! system.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::path::Path;

use whyshelf_core::app::shelf::{
    AtSide, ByteBound, DirListing, Entries, EntryKind, Leads, NameAnswers, NotADir, Side,
    StateList, StateRead, TreeFiles,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{ContentAddress, Untold};

use super::{
    Followed, Leave, MOST_LINKS, PartName, follow, follow_dir, named_as_written, part_names_entry,
};
use crate::shelf::state::files::FileStates;

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

/// A tree as an adapter would answer it: a file's bytes, a directory's entries, and the
/// links, each by the first part of a name it is (the walk asks the rest again).
#[derive(Default)]
struct Tree {
    files: BTreeMap<RelPath, Vec<u8>>,
    dirs: BTreeMap<Option<RelPath>, Vec<(&'static str, EntryKind)>>,
    links: BTreeMap<RelPath, Leads<RelPath>>,
    /// What a reading through a link out of the tree reads.
    outside: BTreeMap<RelPath, Vec<u8>>,
}

impl Tree {
    /// The first part of `name` that is a link, with where it leads.
    fn link_on(&self, name: &RelPath) -> Option<(RelPath, Leads<RelPath>)> {
        let parts: Vec<&str> = name.as_str().split('/').collect();
        (1..=parts.len()).find_map(|n| {
            let at = rel(&parts[..n].join("/"));
            self.links.get(&at).map(|leads| (at, leads.clone()))
        })
    }
}

impl StateRead<RelPath> for Tree {
    type Error = Infallible;

    async fn read(
        &self,
        _: Side,
        names: &BTreeSet<RelPath>,
        _: ByteBound,
    ) -> Result<NameAnswers<RelPath, Vec<u8>, Infallible>, Infallible> {
        Ok(names
            .iter()
            .map(|name| {
                let answer = if let Some((at, leads)) = self.link_on(name) {
                    AtSide::Elsewhere {
                        at,
                        target: "t".to_owned(),
                        leads,
                    }
                } else if let Some(bytes) = self.files.get(name) {
                    AtSide::There(bytes.clone())
                } else if self.dirs.contains_key(&Some(name.clone())) {
                    AtSide::Dir
                } else {
                    AtSide::Absent
                };
                (name.clone(), Ok(answer))
            })
            .collect())
    }

    async fn look(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
    ) -> Result<NameAnswers<RelPath, u64, Infallible>, Infallible> {
        let read = self.read(side, names, ByteBound::WHOLE).await?;
        Ok(read
            .into_iter()
            .map(|(name, answer)| {
                let Ok(answer) = answer;
                let looked = match answer {
                    AtSide::There(bytes) => AtSide::There(bytes.len() as u64),
                    AtSide::Absent => AtSide::Absent,
                    AtSide::Dir => AtSide::Dir,
                    AtSide::Elsewhere { at, target, leads } => {
                        AtSide::Elsewhere { at, target, leads }
                    }
                    AtSide::Untold(why) => AtSide::Untold(why),
                };
                (name, Ok(looked))
            })
            .collect())
    }

    async fn read_outside(
        &self,
        _: Side,
        names: &BTreeSet<RelPath>,
        _: ByteBound,
    ) -> Result<NameAnswers<RelPath, Vec<u8>, Infallible>, Infallible> {
        Ok(names
            .iter()
            .map(|name| {
                let answer = self
                    .outside
                    .get(name)
                    .map_or(AtSide::Absent, |bytes| AtSide::There(bytes.clone()));
                (name.clone(), Ok(answer))
            })
            .collect())
    }

    async fn addresses(
        &self,
        _: Side,
        _: &BTreeSet<RelPath>,
    ) -> Result<BTreeMap<RelPath, ContentAddress>, Infallible> {
        Ok(BTreeMap::new())
    }
}

impl StateList<RelPath> for Tree {
    async fn list(
        &self,
        _: Side,
        dir: Option<&RelPath>,
    ) -> Result<DirListing<RelPath>, Infallible> {
        if let Some((at, leads)) = dir.and_then(|dir| self.link_on(dir)) {
            return Ok(DirListing::NotADir(NotADir::Elsewhere {
                at,
                target: "t".to_owned(),
                leads,
            }));
        }
        let Some(entries) = self.dirs.get(&dir.cloned()) else {
            return Ok(DirListing::NotADir(NotADir::Absent));
        };
        let mut listed = Entries::default();
        for (name, kind) in entries {
            let whole = match dir {
                Some(dir) => format!("{dir}/{name}"),
                None => (*name).to_owned(),
            };
            listed.named.insert(rel(&whole), *kind);
        }
        Ok(DirListing::All(listed))
    }

    async fn list_outside(&self, _: Side, _: &RelPath) -> Result<DirListing<RelPath>, Infallible> {
        Ok(DirListing::NotADir(NotADir::Untold(Untold::NoAnswer)))
    }

    async fn files(&self, _: Side, _: ByteBound) -> Result<TreeFiles<RelPath>, Infallible> {
        Ok(TreeFiles::CannotList(NonBlank::new("not asked").unwrap()))
    }
}

async fn walked(tree: &Tree, name: &str, leave: Leave) -> Followed<RelPath, AtSide<RelPath>> {
    follow(tree, Side::After, &rel(name), leave, ByteBound::WHOLE)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_link_inside_is_read_again_where_it_leads_whatever_the_rule() {
    let mut tree = Tree::default();
    tree.files.insert(rel("real/a.txt"), b"one".to_vec());
    tree.links.insert(rel("near"), Leads::Inside(rel("real")));
    tree.links.insert(rel("here"), Leads::Top);
    for leave in [Leave::Allowed, Leave::Refused] {
        assert_eq!(
            walked(&tree, "near/a.txt", leave).await,
            Followed::Reached {
                at: Some(rel("real/a.txt")),
                answer: AtSide::There(b"one".to_vec())
            }
        );
        assert_eq!(
            walked(&tree, "here/real/a.txt", leave).await,
            Followed::Reached {
                at: Some(rel("real/a.txt")),
                answer: AtSide::There(b"one".to_vec())
            }
        );
        assert_eq!(
            walked(&tree, "here", leave).await,
            Followed::Reached {
                at: None,
                answer: AtSide::Dir
            }
        );
    }
}

#[tokio::test]
async fn a_link_out_or_nowhere_is_read_through_or_refused_by_the_rule_of_the_reader() {
    let mut tree = Tree::default();
    tree.links.insert(rel("out"), Leads::Outside);
    tree.links.insert(rel("broken"), Leads::Broken);
    tree.outside.insert(rel("out/a.txt"), b"away".to_vec());
    assert_eq!(
        walked(&tree, "out/a.txt", Leave::Allowed).await,
        Followed::Reached {
            at: Some(rel("out/a.txt")),
            answer: AtSide::There(b"away".to_vec())
        }
    );
    assert_eq!(
        walked(&tree, "out/a.txt", Leave::Refused).await,
        Followed::Left
    );
    assert_eq!(
        walked(&tree, "broken", Leave::Allowed).await,
        Followed::Reached {
            at: Some(rel("broken")),
            answer: AtSide::Absent
        }
    );
    assert_eq!(
        walked(&tree, "broken", Leave::Refused).await,
        Followed::Broken
    );
}

#[tokio::test]
async fn a_chain_of_links_past_the_most_stops() {
    let mut tree = Tree::default();
    // `l0 -> l1 -> ... `: each read again finds the next.
    for n in 0..=MOST_LINKS {
        tree.links.insert(
            rel(&format!("l{n}")),
            Leads::Inside(rel(&format!("l{}", n + 1))),
        );
    }
    assert_eq!(walked(&tree, "l0", Leave::Allowed).await, Followed::TooMany);
}

#[tokio::test]
async fn a_directory_is_listed_where_a_link_on_its_way_leads() {
    let mut tree = Tree::default();
    tree.dirs
        .insert(Some(rel("real")), vec![("a.txt", EntryKind::File)]);
    tree.links.insert(rel("near"), Leads::Inside(rel("real")));
    tree.links.insert(rel("out"), Leads::Outside);
    let Followed::Reached {
        at,
        answer: DirListing::All(entries),
    } = follow_dir(&tree, Side::After, Some(&rel("near")), Leave::Refused)
        .await
        .unwrap()
    else {
        panic!("a directory");
    };
    assert_eq!(at, Some(rel("real")));
    assert_eq!(
        entries.named.keys().collect::<Vec<_>>(),
        [&rel("real/a.txt")]
    );
    assert_eq!(
        follow_dir(&tree, Side::After, Some(&rel("out")), Leave::Refused)
            .await
            .unwrap(),
        Followed::Left
    );
}

#[tokio::test]
async fn a_name_written_in_another_case_than_its_entry_is_not_as_written() {
    let mut tree = Tree::default();
    tree.dirs.insert(None, vec![("Docs", EntryKind::Dir)]);
    tree.dirs
        .insert(Some(rel("Docs")), vec![("a.md", EntryKind::File)]);
    assert!(named_as_written(&tree, Side::After, &rel("Docs/a.md")).await);
    assert!(!named_as_written(&tree, Side::After, &rel("docs/a.md")).await);
    // A name no entry has in any case (a form the system stores otherwise) says
    // nothing against it.
    assert!(named_as_written(&tree, Side::After, &rel("Docs/b.md")).await);
}

#[test]
fn a_part_is_held_against_the_names_of_its_directory() {
    let names = |all: &'static [&'static str]| all.iter().copied();
    assert_eq!(
        part_names_entry("Justfile", names(&["a", "Justfile"])),
        PartName::Written
    );
    assert_eq!(
        part_names_entry("justfile", names(&["a", "Justfile"])),
        PartName::OtherCase
    );
    // Both spellings there: the one written is a file of its own.
    assert_eq!(
        part_names_entry("justfile", names(&["Justfile", "justfile"])),
        PartName::Written
    );
    // A name stored decomposed (e + U+0301) against the composed one written.
    assert_eq!(
        part_names_entry("caf\u{e9}", names(&["cafe\u{301}"])),
        PartName::Unknown
    );
    assert_eq!(part_names_entry("x", names(&[])), PartName::Unknown);
}

#[cfg(unix)]
#[tokio::test]
async fn over_the_file_system_a_link_to_a_file_inside_is_read_as_its_target() {
    let root = tempfile::tempdir().unwrap();
    let at = |p: &str| root.path().join(p);
    tokio::fs::create_dir_all(at("real")).await.unwrap();
    tokio::fs::write(at("real/a.txt"), "one").await.unwrap();
    tokio::fs::symlink(Path::new("real"), at("near"))
        .await
        .unwrap();
    let state = FileStates::new(root.path().to_path_buf());
    assert_eq!(
        follow(
            &state,
            Side::After,
            &rel("near/a.txt"),
            Leave::Refused,
            ByteBound::WHOLE
        )
        .await
        .unwrap(),
        Followed::Reached {
            at: Some(rel("real/a.txt")),
            answer: AtSide::There(b"one".to_vec())
        }
    );
}
