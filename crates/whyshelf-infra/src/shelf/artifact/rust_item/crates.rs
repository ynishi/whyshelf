//! The crates of the project at a side, read from its manifests: the directory of the
//! project the source of each crate is under, so that a page of the source in the docs
//! (`src/<crate>/<path>.html`, rustdoc's name of a file by its crate and its path under
//! the directory of the crate's root file) names a file of the project.
//!
//! | what | what it does |
//! |---|---|
//! | `Crates::of` | reads the manifest at the root (`Cargo.toml`): its package, and each member of its workspace by the member's own manifest |
//! | `Crates::file_of` | the file of the project a page of the source in the docs is of, or why it cannot be told |
//!
//! The rule of which manifests are read, which crates a package has and where the
//! source of each is, is on the page of the space, "Where a name lives"
//! ([`rust_item`](super)); this module is where it is carried out. A manifest is
//! parsed as TOML. What could not be read (a manifest, a member that is a pattern this
//! module does not read or a directory that is a symbolic link, a directory of a
//! package's source that cannot be listed) is kept with why, and a crate not found
//! then cannot be told, rather than not one of the project; a directory that is not
//! there is no such failure, only nothing there.
//!
//! This module reads the project through its state alone, as every reader does.

use std::collections::{BTreeMap, BTreeSet};

use whyshelf_core::app::shelf::{DirListing, EntryKind, NotADir, Side, StateList};
use whyshelf_core::domain::location::RelPath;

use super::super::super::state::files::FileStates;
use super::super::file::{self, Bounded};
use super::MAX_SETTINGS_BYTES;
use super::docs_path::DocsPath;

/// The name rustdoc gives the directory of the pages of the source, at the top of the
/// docs.
const SOURCE_DIR: &str = "src";

/// The part rustdoc writes in the path of a page of the source for `..`.
const UP: &str = "up";

/// The name of a manifest.
const MANIFEST: &str = "Cargo.toml";

/// The parts of a directory of the project, from its root (none: the root).
type Dir = Vec<String>;

/// Where the source of a crate is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Root {
    /// Under this directory: the directory of the crate's root file.
    Under(Dir),
    /// Two packages have a library of the name, under these directories.
    Two(Dir, Dir),
}

/// The crates of the project at one side, by the name rustdoc gives each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Crates {
    /// The libraries, by name.
    libraries: BTreeMap<String, Root>,
    /// The binaries, by name: a library of the same name is the crate.
    binaries: BTreeMap<String, Root>,
    /// What could not be read, each with why: a manifest, a member, a directory of a
    /// package's source.
    unread: Vec<String>,
}

impl Crates {
    /// The crates of the project at `side`, read from the manifest at its root and
    /// those of the members of its workspace.
    pub(super) async fn of(state: &FileStates, side: Side) -> Self {
        let mut crates = Self::default();
        let Some(root) = crates.manifest(state, side, &Dir::new(), true).await else {
            return crates;
        };
        let workspace = root.get("workspace").and_then(toml::Value::as_table);
        // The edition a member inherits with `edition.workspace = true`.
        let inherited = workspace
            .and_then(|w| w.get("package"))
            .and_then(|p| p.get("edition"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        crates
            .read_package(state, side, &root, &Dir::new(), inherited.as_deref())
            .await;
        let Some(workspace) = workspace else {
            return crates;
        };
        let texts = |key: &str| -> Vec<String> {
            workspace
                .get(key)
                .and_then(toml::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let excluded: Vec<Dir> = texts("exclude").iter().map(|t| parts_of(t)).collect();
        let mut members: BTreeSet<Dir> = BTreeSet::new();
        for member in texts("members") {
            let pattern = parts_of(&member);
            // A member written as a path is a member whatever `exclude` says; what a
            // pattern matches is not, under a path `exclude` names.
            if !pattern.iter().any(|part| is_pattern(part)) {
                members.insert(pattern);
                continue;
            }
            match crates.expand(state, side, &member).await {
                Ok(dirs) => members.extend(
                    dirs.into_iter()
                        .filter(|dir| !excluded.iter().any(|out| dir.starts_with(out))),
                ),
                Err(why) => crates.unread.push(why),
            }
        }
        members.remove(&Dir::new());
        for dir in &members {
            if let Some(manifest) = crates.manifest(state, side, dir, false).await {
                crates
                    .read_package(state, side, &manifest, dir, inherited.as_deref())
                    .await;
            }
        }
        crates
    }

    /// The file of the project the page of the source `page` is of: the part after
    /// `src/<crate>/` and before `.html`, under the directory of the crate's source,
    /// or why it cannot be told.
    pub(super) fn file_of(&self, page: &DocsPath) -> Result<RelPath, String> {
        let parts: Vec<&str> = page.parts().collect();
        let (krate, below) = match parts.as_slice() {
            [top, krate, below @ ..] if *top == SOURCE_DIR && !below.is_empty() => (*krate, below),
            _ => {
                return Err(format!(
                    "the docs link to no page of the source: {}",
                    joined(&parts)
                ));
            }
        };
        let root = self
            .libraries
            .get(krate)
            .or_else(|| self.binaries.get(krate));
        let dir = match root {
            Some(Root::Under(dir)) => dir,
            Some(Root::Two(one, two)) => {
                return Err(format!(
                    "two packages of the project have a crate `{krate}`, under {} and {}",
                    shown(one),
                    shown(two)
                ));
            }
            None if self.unread.is_empty() => {
                return Err(format!(
                    "the crate `{krate}` is no crate of the project's manifests (a dependency, or docs left by an earlier build)"
                ));
            }
            None => {
                return Err(format!(
                    "the crate `{krate}` is no crate of the manifests read, and some could not be: {}",
                    self.unread.join("; ")
                ));
            }
        };
        let mut file = dir.clone();
        for (at, part) in below.iter().enumerate() {
            let part = match (at + 1 == below.len(), part.strip_suffix(".html")) {
                (true, Some(name)) => name,
                (true, None) => {
                    return Err(format!(
                        "the docs link to no page of the source: {}",
                        joined(&parts)
                    ));
                }
                (false, _) => part,
            };
            if part == UP {
                if file.pop().is_none() {
                    return Err(format!(
                        "the source of the crate `{krate}` leads out of the project: {}",
                        joined(&parts)
                    ));
                }
            } else {
                file.push(part.to_owned());
            }
        }
        file.join("/").parse().map_err(|e| {
            format!("the source of the crate `{krate}` is no path of the project: {e}")
        })
    }

    /// The manifest of the directory `dir`, parsed, or none (with why, kept, when it
    /// is there and cannot be read, or when `needed` and it is not there).
    async fn manifest(
        &mut self,
        state: &FileStates,
        side: Side,
        dir: &Dir,
        needed: bool,
    ) -> Option<toml::Table> {
        let mut at = dir.clone();
        at.push(MANIFEST.to_owned());
        let shown = at.join("/");
        let Ok(rel) = shown.parse::<RelPath>() else {
            self.unread
                .push(format!("{shown} is no path of the project"));
            return None;
        };
        let bytes = match file::read_within(state, side, &rel, MAX_SETTINGS_BYTES).await {
            Ok(Bounded::Bytes(bytes)) => bytes,
            Ok(Bounded::NoFile) => {
                if needed {
                    self.unread.push(format!("the project has no {shown}"));
                }
                return None;
            }
            Ok(Bounded::Outside) => {
                self.unread.push(format!(
                    "{shown} leads outside the root of the project, or is broken"
                ));
                return None;
            }
            Ok(Bounded::TooLarge) => {
                self.unread
                    .push(format!("{shown} is larger than {MAX_SETTINGS_BYTES} bytes"));
                return None;
            }
            Ok(Bounded::Untold(why)) => {
                self.unread.push(format!("{shown} cannot be read: {why}"));
                return None;
            }
            Err(e) => {
                self.unread.push(format!("{shown} cannot be read: {e}"));
                return None;
            }
        };
        let parsed = String::from_utf8(bytes)
            .map_err(|_| format!("{shown} is not UTF-8 text"))
            .and_then(|text| {
                text.parse::<toml::Table>()
                    .map_err(|e| format!("{shown} is not TOML: {e}"))
            });
        match parsed {
            Ok(table) => Some(table),
            Err(why) => {
                self.unread.push(why);
                None
            }
        }
    }

    /// Whether the package the manifest `manifest` of `dir` declares has a library, as
    /// cargo tells it: its `[lib]`; else, unless `autolib = false`, a file
    /// `src/lib.rs`.
    async fn has_library(
        &mut self,
        state: &FileStates,
        side: Side,
        manifest: &toml::Table,
        dir: &Dir,
    ) -> bool {
        if manifest.get("lib").is_some() {
            return true;
        }
        if package_flag(manifest, "autolib") == Some(false) {
            return false;
        }
        self.files_under(state, side, &under(dir, &[SOURCE_DIR]))
            .await
            .contains("lib.rs")
    }

    /// The crates of the package the manifest `manifest` of `dir` declares, if it
    /// declares one, by the rule on the page of the space: its library
    /// ([`Self::has_library`]), and its binaries, those its layout holds
    /// ([`Self::layout_bins`]) when cargo finds them, with each declared one
    /// (`[[bin]]`) in the place of one of its name. `inherited` is the edition of the
    /// workspace, for a package that inherits it.
    async fn read_package(
        &mut self,
        state: &FileStates,
        side: Side,
        manifest: &toml::Table,
        dir: &Dir,
        inherited: Option<&str>,
    ) {
        let Some(name) = package_name(manifest) else {
            return;
        };
        let library = self.has_library(state, side, manifest, dir).await;
        let text = |table: &toml::Value, key: &str| {
            table
                .get(key)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        };
        let declared: Vec<(String, Option<String>)> = manifest
            .get("bin")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|bin| Some((text(bin, "name")?, text(bin, "path"))))
            .collect();
        let layout = self.layout_bins(state, side, dir, name).await;
        // Cargo finds binaries by the layout unless told not to; of the edition 2015, a
        // package that declares one finds none unless told to.
        let edition = match manifest.get("package").and_then(|p| p.get("edition")) {
            Some(toml::Value::String(edition)) => Some(edition.as_str()),
            Some(_) => inherited,
            None => None,
        };
        let auto = package_flag(manifest, "autobins")
            .unwrap_or(edition.unwrap_or("2015") != "2015" || declared.is_empty());
        let mut bins: BTreeMap<String, String> = match auto {
            true => layout.clone(),
            false => BTreeMap::new(),
        };
        for (bin_name, path) in declared {
            let path = path
                .or_else(|| layout.get(&bin_name).cloned())
                .unwrap_or_else(|| match bin_name == name {
                    true => "src/main.rs".to_owned(),
                    false => format!("src/bin/{bin_name}.rs"),
                });
            bins.insert(bin_name, path);
        }
        self.package(manifest, dir, library, bins);
    }

    /// The binaries the layout of the package `name` at `dir` holds, by name, with
    /// their paths: `src/main.rs` by the name of the package, and each
    /// `src/bin/<name>.rs` and `src/bin/<name>/main.rs` by its name.
    async fn layout_bins(
        &mut self,
        state: &FileStates,
        side: Side,
        dir: &Dir,
        name: &str,
    ) -> BTreeMap<String, String> {
        let mut found = BTreeMap::new();
        if self
            .files_under(state, side, &under(dir, &[SOURCE_DIR]))
            .await
            .contains("main.rs")
        {
            found.insert(name.to_owned(), "src/main.rs".to_owned());
        }
        let bin = under(dir, &[SOURCE_DIR, "bin"]);
        for file in self.files_under(state, side, &bin).await {
            if let Some(stem) = file.strip_suffix(".rs") {
                found.insert(stem.to_owned(), format!("src/bin/{file}"));
            }
        }
        for sub in self.dirs_under(state, side, &bin).await {
            let at = under(&bin, &[sub.as_str()]);
            if self.files_under(state, side, &at).await.contains("main.rs") {
                found
                    .entry(sub.clone())
                    .or_insert_with(|| format!("src/bin/{sub}/main.rs"));
            }
        }
        found
    }

    /// The names of the files under `dir`: none when it is not there, and none, with
    /// why kept as not read, when it cannot be listed.
    async fn files_under(&mut self, state: &FileStates, side: Side, dir: &Dir) -> BTreeSet<String> {
        match self.subfiles(state, side, dir).await {
            Ok(files) => files,
            Err(why) => {
                self.unread.push(why);
                BTreeSet::new()
            }
        }
    }

    /// The names of the directories under `dir`, as [`Self::files_under`] lists files.
    async fn dirs_under(&mut self, state: &FileStates, side: Side, dir: &Dir) -> Vec<String> {
        match self.subdirs(state, side, dir).await {
            Ok(dirs) => dirs,
            Err(why) => {
                self.unread.push(why);
                Vec::new()
            }
        }
    }

    /// The crates of the package the manifest `manifest` of `dir` declares: its library
    /// when it has one (`library`), and its binaries (`bins`, by name, with their
    /// paths).
    fn package(
        &mut self,
        manifest: &toml::Table,
        dir: &Dir,
        library: bool,
        bins: BTreeMap<String, String>,
    ) {
        let Some(name) = package_name(manifest) else {
            return;
        };
        let text = |table: &toml::Value, key: &str| {
            table
                .get(key)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        };
        let lib = manifest.get("lib");
        let lib_name = lib
            .and_then(|l| text(l, "name"))
            .unwrap_or_else(|| name.replace('-', "_"));
        let lib_path = lib
            .and_then(|l| text(l, "path"))
            .unwrap_or_else(|| "src/lib.rs".to_owned());
        if let Some(root) = root_of(dir, &lib_path).filter(|_| library) {
            put(&mut self.libraries, lib_name, root);
        }
        for (bin_name, path) in bins {
            if let Some(root) = root_of(dir, &path) {
                put(&mut self.binaries, bin_name.replace('-', "_"), root);
            }
        }
    }

    /// The directories a member of the workspace names: itself when it is a path,
    /// and each directory its pattern matches that holds a manifest when it is one.
    async fn expand(
        &mut self,
        state: &FileStates,
        side: Side,
        member: &str,
    ) -> Result<Vec<Dir>, String> {
        let pattern = parts_of(member);
        if pattern.iter().any(|part| part == "**") {
            return Err(format!(
                "the member `{member}` of the workspace holds `**`, which is not read"
            ));
        }
        if pattern.iter().any(|part| part.contains('[')) {
            return Err(format!(
                "the member `{member}` of the workspace holds `[`, which is not read"
            ));
        }
        if !pattern.iter().any(|part| is_pattern(part)) {
            return Ok(vec![pattern]);
        }
        let mut dirs: Vec<Dir> = vec![Dir::new()];
        for part in &pattern {
            let mut next = Vec::new();
            for dir in dirs {
                if !is_pattern(part) {
                    let mut under = dir;
                    under.push(part.clone());
                    next.push(under);
                    continue;
                }
                for entry in self.subdirs(state, side, &dir).await? {
                    if matches(part, &entry) {
                        next.push(under(&dir, &[entry.as_str()]));
                    }
                }
                // A directory reached through a link is not walked: what it holds
                // cannot be told, and is said.
                for link in self.entries(state, side, &dir, EntryKind::Link).await? {
                    if matches(part, &link) {
                        self.unread.push(format!(
                            "the member {} of the workspace is a symbolic link, which is not followed",
                            under(&dir, &[link.as_str()]).join("/")
                        ));
                    }
                }
            }
            dirs = next;
        }
        let mut found = Vec::new();
        for dir in dirs {
            if self.subfiles(state, side, &dir).await?.contains(MANIFEST) {
                found.push(dir);
            }
        }
        Ok(found)
    }

    /// The names of the directories under `dir`, as the state lists them.
    async fn subdirs(
        &self,
        state: &FileStates,
        side: Side,
        dir: &Dir,
    ) -> Result<Vec<String>, String> {
        self.entries(state, side, dir, EntryKind::Dir).await
    }

    /// The names of the files under `dir`, as the state lists them.
    async fn subfiles(
        &self,
        state: &FileStates,
        side: Side,
        dir: &Dir,
    ) -> Result<BTreeSet<String>, String> {
        Ok(self
            .entries(state, side, dir, EntryKind::File)
            .await?
            .into_iter()
            .collect())
    }

    /// The names of the entries of `kind` under `dir`; none when it is no directory.
    async fn entries(
        &self,
        state: &FileStates,
        side: Side,
        dir: &Dir,
        kind: EntryKind,
    ) -> Result<Vec<String>, String> {
        let at = match dir.is_empty() {
            true => None,
            false => Some(
                dir.join("/")
                    .parse::<RelPath>()
                    .map_err(|e| format!("{} is no path of the project: {e}", dir.join("/")))?,
            ),
        };
        match state.list(side, at.as_ref()).await {
            Ok(DirListing::All(entries)) => Ok(entries
                .named
                .into_iter()
                .filter(|(_, of)| *of == kind)
                .filter_map(|(name, _)| name.as_str().rsplit('/').next().map(str::to_owned))
                .collect()),
            Ok(DirListing::Cut { why, .. }) => Err(format!(
                "the directory {} of the workspace is not listed whole: {why}",
                shown(dir)
            )),
            // Not there: nothing; anything else that is no directory cannot be told.
            Ok(DirListing::NotADir(NotADir::Absent | NotADir::File)) => Ok(Vec::new()),
            Ok(DirListing::NotADir(NotADir::Elsewhere { .. })) => Err(format!(
                "{} is reached through a symbolic link, which is not followed",
                shown(dir)
            )),
            Ok(DirListing::NotADir(NotADir::Untold(why))) => {
                Err(format!("{} cannot be told: {why}", shown(dir)))
            }
            Err(e) => Err(format!(
                "the directory {} cannot be listed: {e}",
                shown(dir)
            )),
        }
    }
}

/// `dir` with `parts` under it.
fn under(dir: &Dir, parts: &[&str]) -> Dir {
    let mut at = dir.clone();
    at.extend(parts.iter().map(|part| (*part).to_owned()));
    at
}

/// The value of a flag of the `[package]` a manifest declares (`autolib`,
/// `autobins`), when it writes one.
fn package_flag(manifest: &toml::Table, flag: &str) -> Option<bool> {
    manifest
        .get("package")
        .and_then(|p| p.get(flag))
        .and_then(toml::Value::as_bool)
}

/// The name of the package a manifest declares, if it declares one.
fn package_name(manifest: &toml::Table) -> Option<&str> {
    manifest
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
}

/// Puts the crate `name` under `root`, or tells two of one name apart.
fn put(crates: &mut BTreeMap<String, Root>, name: String, root: Dir) {
    match crates.get(&name) {
        None => {
            crates.insert(name, Root::Under(root));
        }
        Some(Root::Under(dir)) if *dir == root => {}
        Some(Root::Under(dir)) => {
            let two = Root::Two(dir.clone(), root);
            crates.insert(name, two);
        }
        Some(Root::Two(..)) => {}
    }
}

/// The directory of the root file `path` of a crate of the package at `dir`, with
/// `.` and `..` read; none when it leads out of the project.
fn root_of(dir: &Dir, path: &str) -> Option<Dir> {
    let mut at = dir.clone();
    let parts = parts_of(path);
    let (_, above) = parts.split_last()?;
    for part in above {
        match part.as_str() {
            ".." => {
                at.pop()?;
            }
            part => at.push(part.to_owned()),
        }
    }
    Some(at)
}

/// The parts of a path as a manifest writes it: `/` and `\` between them, the empty
/// and `.` ones left out.
fn parts_of(text: &str) -> Dir {
    text.split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_owned)
        .collect()
}

/// Whether a part of a member is a pattern.
fn is_pattern(part: &str) -> bool {
    part.contains(['*', '?', '['])
}

/// Whether the name `name` matches the pattern `part`: `*` any run of characters,
/// `?` any one, every other character itself. A member with `[` is not read
/// (`Crates::expand` tells it so), and never comes here.
fn matches(part: &str, name: &str) -> bool {
    let (pattern, name): (Vec<char>, Vec<char>) = (part.chars().collect(), name.chars().collect());
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some('?') => {
                p += 1;
                n += 1;
            }
            Some(c) if *c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((at, from)) => {
                    p = at + 1;
                    n = from + 1;
                    star = Some((at, from + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// A directory of the project as a reason writes it.
fn shown(dir: &Dir) -> String {
    match dir.is_empty() {
        true => "the root".to_owned(),
        false => dir.join("/"),
    }
}

/// The parts of a place in the docs, as a reason writes them.
fn joined(parts: &[&str]) -> String {
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::fixture::write;
    use super::*;

    fn page(text: &str) -> DocsPath {
        DocsPath::of_relative(Path::new(text)).unwrap()
    }

    fn file(text: &str) -> RelPath {
        text.parse().unwrap()
    }

    async fn crates_at(root: &Path) -> Crates {
        Crates::of(&FileStates::new(root.to_path_buf()), Side::After).await
    }

    #[test]
    fn a_part_of_a_member_matches_by_its_stars_and_marks() {
        assert!(matches("*", "whyshelf-core"));
        assert!(matches("whyshelf-*", "whyshelf-core"));
        assert!(matches("?ore", "core"));
        assert!(matches("a*b*c", "aXbYc"));
        assert!(!matches("a*b", "aXc"));
        assert!(!matches("core?", "core"));
    }

    #[tokio::test]
    async fn a_package_at_the_root_has_its_library_and_its_binary() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), MANIFEST, "[package]\nname = \"my-demo\"\n").await;
        write(dir.path(), "src/lib.rs", "pub mod a;\n").await;
        let crates = crates_at(dir.path()).await;
        assert_eq!(
            crates.file_of(&page("src/my_demo/lib.rs.html")),
            Ok(file("src/lib.rs"))
        );
        assert_eq!(
            crates.file_of(&page("src/my_demo/a/b.rs.html")),
            Ok(file("src/a/b.rs"))
        );
        // A part `up` is rustdoc's `..`.
        assert_eq!(
            crates.file_of(&page("src/my_demo/up/build.rs.html")),
            Ok(file("build.rs"))
        );
        assert!(
            crates
                .file_of(&page("src/my_demo/up/up/x.rs.html"))
                .unwrap_err()
                .contains("out of the project")
        );
        assert!(
            crates
                .file_of(&page("src/serde/lib.rs.html"))
                .unwrap_err()
                .contains("no crate of the project")
        );
        assert!(crates.file_of(&page("my_demo/struct.A.html")).is_err());
    }

    #[tokio::test]
    async fn a_package_with_no_library_is_its_binaries_alone() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            MANIFEST,
            "[package]\nname = \"tool\"\n[[bin]]\nname = \"tool\"\npath = \"cli/main.rs\"\n",
        )
        .await;
        write(dir.path(), "src/util.rs", "pub fn x() {}\n").await;
        write(dir.path(), "cli/main.rs", "fn main() {}\n").await;
        let crates = crates_at(dir.path()).await;
        // No `[lib]` and no `src/lib.rs`: the crate `tool` is the binary, under `cli`.
        assert_eq!(
            crates.file_of(&page("src/tool/main.rs.html")),
            Ok(file("cli/main.rs"))
        );
        // With a `src/lib.rs`, the library of the name is the crate.
        write(dir.path(), "src/lib.rs", "pub mod util;\n").await;
        let crates = crates_at(dir.path()).await;
        assert_eq!(
            crates.file_of(&page("src/tool/util.rs.html")),
            Ok(file("src/util.rs"))
        );
    }

    #[tokio::test]
    async fn the_binaries_the_layout_holds_are_found_beside_those_declared_unless_autobins_is_off()
    {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nedition = \"2021\"\n[[bin]]\nname = \"extra\"\npath = \"tools/extra.rs\"\n",
        )
        .await;
        write(root, "src/main.rs", "fn main() {}\n").await;
        write(root, "src/bin/one.rs", "fn main() {}\n").await;
        write(root, "src/bin/two/main.rs", "fn main() {}\n").await;
        write(root, "src/bin/two/part.rs", "pub fn x() {}\n").await;
        write(root, "tools/extra.rs", "fn main() {}\n").await;
        let crates = crates_at(root).await;
        let of = |crates: &Crates, text: &str| crates.file_of(&page(text));
        // A `[[bin]]` declared, and still `src/main.rs` and `src/bin/*` by the layout.
        assert_eq!(
            of(&crates, "src/extra/extra.rs.html"),
            Ok(file("tools/extra.rs"))
        );
        assert_eq!(of(&crates, "src/kit/main.rs.html"), Ok(file("src/main.rs")));
        assert_eq!(
            of(&crates, "src/one/one.rs.html"),
            Ok(file("src/bin/one.rs"))
        );
        assert_eq!(
            of(&crates, "src/two/part.rs.html"),
            Ok(file("src/bin/two/part.rs"))
        );
        // A declared binary of a found one's name takes its place.
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nedition = \"2021\"\n[[bin]]\nname = \"one\"\npath = \"tools/extra.rs\"\n",
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/one/extra.rs.html"),
            Ok(file("tools/extra.rs"))
        );
        // `autobins = false`: only what is declared.
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nautobins = false\n[[bin]]\nname = \"extra\"\npath = \"tools/extra.rs\"\n",
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/extra/extra.rs.html"),
            Ok(file("tools/extra.rs"))
        );
        for gone in ["src/kit/main.rs.html", "src/one/one.rs.html"] {
            assert!(of(&crates, gone).is_err(), "{gone}");
        }
        // A declared binary with no path, `autobins = false`: its path is still the
        // layout's, `src/bin/<name>/main.rs` among them.
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nedition = \"2021\"\nautobins = false\n[[bin]]\nname = \"two\"\n",
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/two/part.rs.html"),
            Ok(file("src/bin/two/part.rs"))
        );
        assert!(of(&crates, "src/one/one.rs.html").is_err());
    }

    #[tokio::test]
    async fn of_the_edition_2015_a_declared_binary_turns_the_layout_off_unless_autobins_says() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/main.rs", "fn main() {}\n").await;
        write(root, "src/bin/one.rs", "fn main() {}\n").await;
        write(root, "tools/extra.rs", "fn main() {}\n").await;
        let declared = "[[bin]]\nname = \"extra\"\npath = \"tools/extra.rs\"\n";
        let of = |crates: &Crates, text: &str| crates.file_of(&page(text));
        // No edition written is 2015: only what is declared.
        write(
            root,
            MANIFEST,
            &format!("[package]\nname = \"kit\"\n{declared}"),
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/extra/extra.rs.html"),
            Ok(file("tools/extra.rs"))
        );
        assert!(of(&crates, "src/kit/main.rs.html").is_err());
        assert!(of(&crates, "src/one/one.rs.html").is_err());
        // `autobins = true` finds them again; so does the edition 2018 and later.
        for package in [
            "[package]\nname = \"kit\"\nautobins = true\n",
            "[package]\nname = \"kit\"\nedition = \"2018\"\n",
        ] {
            write(root, MANIFEST, &format!("{package}{declared}")).await;
            let crates = crates_at(root).await;
            assert_eq!(
                of(&crates, "src/one/one.rs.html"),
                Ok(file("src/bin/one.rs")),
                "{package}"
            );
        }
        // An edition inherited from the workspace is the workspace's.
        write(
            root,
            MANIFEST,
            &format!(
                "[workspace]\n[workspace.package]\nedition = \"2021\"\n[package]\nname = \"kit\"\nedition.workspace = true\n{declared}"
            ),
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(of(&crates, "src/kit/main.rs.html"), Ok(file("src/main.rs")));
        // With no binary declared, the layout is read whatever the edition.
        write(root, MANIFEST, "[package]\nname = \"kit\"\n").await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/one/one.rs.html"),
            Ok(file("src/bin/one.rs"))
        );
    }

    #[tokio::test]
    async fn autolib_off_leaves_a_src_lib_rs_no_library() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/lib.rs", "pub fn x() {}\n").await;
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nautolib = false\n",
        )
        .await;
        let crates = crates_at(root).await;
        assert!(crates.file_of(&page("src/kit/lib.rs.html")).is_err());
    }

    #[tokio::test]
    async fn exclude_takes_out_what_is_under_its_paths_and_a_member_written_as_a_path_stays() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[workspace]\nmembers = [\"crates/*\", \"crates/old/kept\", \"vendor/*/*\"]\nexclude = [\"crates/old\", \"vendor/x\"]\n",
        )
        .await;
        for (at, name) in [
            ("crates/new", "new"),
            ("crates/old", "old"),
            ("crates/old/kept", "kept"),
            ("vendor/x/a", "xa"),
            ("vendor/y/b", "yb"),
        ] {
            write(
                root,
                &format!("{at}/Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\n"),
            )
            .await;
            write(root, &format!("{at}/src/lib.rs"), "pub fn x() {}\n").await;
        }
        let crates = crates_at(root).await;
        let of = |text: &str| crates.file_of(&page(text));
        assert_eq!(of("src/new/lib.rs.html"), Ok(file("crates/new/src/lib.rs")));
        assert_eq!(
            of("src/kept/lib.rs.html"),
            Ok(file("crates/old/kept/src/lib.rs"))
        );
        assert_eq!(of("src/yb/lib.rs.html"), Ok(file("vendor/y/b/src/lib.rs")));
        assert!(of("src/old/lib.rs.html").is_err());
        assert!(of("src/xa/lib.rs.html").is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_member_through_a_link_and_a_source_that_cannot_be_listed_are_not_read_and_said() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, MANIFEST, "[workspace]\nmembers = [\"crates/*\"]\n").await;
        write(root, "elsewhere/Cargo.toml", "[package]\nname = \"far\"\n").await;
        write(root, "elsewhere/src/lib.rs", "pub fn x() {}\n").await;
        write(
            root,
            "crates/near/Cargo.toml",
            "[package]\nname = \"near\"\n",
        )
        .await;
        write(root, "crates/near/src/lib.rs", "pub fn x() {}\n").await;
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
        tokio::fs::symlink(root.join("elsewhere"), root.join("crates/far"))
            .await
            .unwrap();
        let crates = crates_at(root).await;
        let why = crates.file_of(&page("src/far/lib.rs.html")).unwrap_err();
        assert!(why.contains("symbolic link"), "{why}");
        // A `src` that cannot be listed is not read, and said: not a library missing.
        let src = root.join("crates/near/src");
        // arch-lint: allow(readers-read-through-the-state) reason="a test locks a directory of its tree"
        tokio::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();
        // arch-lint: allow(readers-read-through-the-state) reason="a test asks whether its tree locks a directory"
        let locked = tokio::fs::read_dir(&src).await.is_err();
        let crates = crates_at(root).await;
        // arch-lint: allow(readers-read-through-the-state) reason="a test unlocks a directory of its tree"
        tokio::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();
        // Run as a user every directory opens to (root), it is no case to test.
        if locked {
            let why = crates.file_of(&page("src/near/lib.rs.html")).unwrap_err();
            assert!(why.contains("could not be"), "{why}");
        }
    }

    #[tokio::test]
    async fn the_members_of_a_workspace_are_read_by_their_manifests_and_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[workspace]\nmembers = [\"crates/*\", \"tool\"]\nexclude = [\"crates/skip\"]\n",
        )
        .await;
        write(
            root,
            "crates/core/Cargo.toml",
            "[package]\nname = \"my-core\"\n",
        )
        .await;
        write(
            root,
            "crates/cli/Cargo.toml",
            "[package]\nname = \"my-cli\"\n[[bin]]\nname = \"my\"\npath = \"src/main.rs\"\n",
        )
        .await;
        write(
            root,
            "crates/lib/Cargo.toml",
            "[package]\nname = \"x\"\n[lib]\nname = \"other\"\npath = \"code/top.rs\"\n",
        )
        .await;
        write(
            root,
            "crates/skip/Cargo.toml",
            "[package]\nname = \"skip\"\n",
        )
        .await;
        write(root, "crates/empty/README", "no manifest\n").await;
        write(root, "tool/Cargo.toml", "[package]\nname = \"tool\"\n").await;
        for lib in [
            "crates/core/src/lib.rs",
            "crates/skip/src/lib.rs",
            "tool/src/lib.rs",
        ] {
            write(root, lib, "pub fn x() {}\n").await;
        }
        let crates = crates_at(root).await;
        let of = |text: &str| crates.file_of(&page(text));
        assert_eq!(
            of("src/my_core/app/x.rs.html"),
            Ok(file("crates/core/src/app/x.rs"))
        );
        assert_eq!(
            of("src/my/main.rs.html"),
            Ok(file("crates/cli/src/main.rs"))
        );
        assert_eq!(
            of("src/other/top.rs.html"),
            Ok(file("crates/lib/code/top.rs"))
        );
        assert_eq!(of("src/tool/lib.rs.html"), Ok(file("tool/src/lib.rs")));
        assert!(of("src/skip/lib.rs.html").is_err());
        assert_eq!(crates.unread, Vec::<String>::new());
    }

    #[tokio::test]
    async fn two_libraries_of_one_name_and_a_manifest_not_read_cannot_be_told() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[workspace]\nmembers = [\"a\", \"b\", \"c/**\", \"d/[xy]\"]\n",
        )
        .await;
        write(root, "a/Cargo.toml", "[package]\nname = \"same\"\n").await;
        write(root, "b/Cargo.toml", "[package]\nname = \"same\"\n").await;
        write(root, "a/src/lib.rs", "pub fn x() {}\n").await;
        write(root, "b/src/lib.rs", "pub fn x() {}\n").await;
        let crates = crates_at(root).await;
        assert!(
            crates
                .file_of(&page("src/same/lib.rs.html"))
                .unwrap_err()
                .contains("two packages")
        );
        assert!(
            crates
                .file_of(&page("src/elsewhere/lib.rs.html"))
                .unwrap_err()
                .contains("`**`")
        );
        // A member with `[` is not read either, and says so.
        assert!(
            crates
                .file_of(&page("src/elsewhere/lib.rs.html"))
                .unwrap_err()
                .contains("`[`")
        );
        // No manifest at the root: nothing is a crate of the project, and it says so.
        let bare = tempfile::tempdir().unwrap();
        let crates = crates_at(bare.path()).await;
        assert!(
            crates
                .file_of(&page("src/demo/lib.rs.html"))
                .unwrap_err()
                .contains("has no Cargo.toml")
        );
    }
}
