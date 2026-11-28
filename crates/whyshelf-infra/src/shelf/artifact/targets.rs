//! The targets of the Rust packages of a project at a side, read from their manifests:
//! the root file of each crate a package builds (its library, each binary, each
//! integration test, each bench and each example), with the name of the crate and the
//! directory of its package. It is the one reader of the manifests of a project: the
//! space `rust-item` takes from it the crate a page of the source in the docs is of,
//! and the space `rust-test` the crates whose modules it reads.
//!
//! | what | what it does |
//! |---|---|
//! | `Targets::of` | reads the manifest at the root (`Cargo.toml`): its package, and each member of its workspace by the member's own manifest |
//! | `Targets::of_project` | the same; and, with no manifest at the root, the manifest of each directory at the top of the tree read the same way: the manifests the space `rust-item` reads |
//! | `Targets::path_dependencies` | the libraries of the project a package depends on by a path, of its own or of its workspace's |
//! | `Targets::crate_named` | the library or the binary a crate's name is: the library first, and two of them when two packages have a crate of the name under two directories |
//!
//! Which manifests are read, and the library and the binaries of a package, are on the
//! page of the space `rust-item`, "Where a name lives" ([`rust_item`](super::rust_item)).
//! The other targets of a package are found as cargo finds them: each `[[test]]`,
//! `[[bench]]` and `[[example]]` declared, and, unless `autotests`, `autobenches` or
//! `autoexamples` says not to (or, of the edition 2015, unless the package declares one
//! of the kind and the flag does not say to), each `<dir>/<name>.rs` and
//! `<dir>/<name>/main.rs` under `tests`, `benches` and `examples`; a declared target of
//! a found one's name takes its place, and one declared with no path is at the found
//! one's, else at `<dir>/<name>.rs`.
//!
//! A manifest is parsed as TOML. What could not be read (a manifest, a member that is
//! a pattern this module does not read or a directory that is a symbolic link, a
//! `workspace.members` or `workspace.exclude` that is no array of strings or has an
//! item that is no string, any other field of a manifest read here that is there in a
//! shape not read (`workspace`, `package`, `package.name`, `package.autolib`, `lib`,
//! `[[bin]]` and the like, a target's `name` or `path`; the one reader is [`Field`], and
//! one that is absent is no such failure), a target whose root leads out of the project or is no path
//! of it, a directory that cannot be listed whole) is kept with why ([`Unread`]), so that a
//! reader can tell a crate not found from one not read; a directory that is not there
//! is no such failure, only nothing there. A project with no manifest at its root has
//! no target, and nothing in it went unread ([`Top::Absent`]).
//!
//! This module reads the project through its state alone, as every reader does.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use toml::Value;

use whyshelf_core::app::shelf::{DirListing, EntryKind, NotADir, Side, StateList};
use whyshelf_core::domain::location::RelPath;

use super::super::state::ProjectTree;
use super::file::{self, Bounded};
use super::rust_item::MAX_SETTINGS_BYTES;

/// The name of a manifest.
pub(super) const MANIFEST: &str = "Cargo.toml";

/// The directory of a package's source, where its library and binaries are found.
const SOURCE_DIR: &str = "src";

/// The parts of a directory of the project, from its root (none: the root).
pub(super) type Dir = Vec<String>;

/// What a target of a package is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// The library.
    Lib,
    /// A binary.
    Bin,
    /// An integration test: every item of it is compiled only under test.
    Test,
    /// A bench.
    Bench,
    /// An example.
    Example,
}

/// The kinds of target cargo finds by the layout of a package beside those declared:
/// the kind, the table that declares one (`[[test]]`), the directory they are found
/// in, and the flag of the package that turns finding them off.
const FOUND_KINDS: &[(Kind, &str, &str, &str)] = &[
    (Kind::Test, "test", "tests", "autotests"),
    (Kind::Bench, "bench", "benches", "autobenches"),
    (Kind::Example, "example", "examples", "autoexamples"),
];

/// One target of a package: what it is, the name of its crate (a library's `[lib]
/// name`, or the name of the package with `-` as `_`; another target's own name, with
/// `-` as `_`), the directory of its package, and its root file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Target {
    pub(super) kind: Kind,
    pub(super) name: String,
    pub(super) package: Dir,
    pub(super) root: RelPath,
}

impl Target {
    /// The directory of its root file, the one rustdoc names its pages of the source
    /// under.
    pub(super) fn root_dir(&self) -> Dir {
        let mut dir = parts_of(self.root.as_str());
        dir.pop();
        dir
    }
}

/// What of the project could not be read, with why: the why is the whole sentence a
/// reason says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Unread {
    /// A manifest that is there and cannot be read, or one that is no path.
    Manifest { path: String, why: String },
    /// A member of the workspace written as a pattern this module does not read, or a
    /// directory a pattern matches that is a symbolic link.
    Member { pattern: String, why: String },
    /// A directory that cannot be listed whole.
    Dir { dir: String, why: String },
}

impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest { why, .. } | Self::Member { why, .. } | Self::Dir { why, .. } => {
                f.write_str(why)
            }
        }
    }
}

/// Whether the project has a manifest at its root.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Top {
    /// It has one, read or not (what could not be read of it is [`Unread`]).
    Read,
    /// It has none: the project has no target, and nothing of it went unread.
    #[default]
    Absent,
}

/// The library or the binary a crate's name is ([`Targets::crate_named`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CrateOf<'a> {
    One(&'a Target),
    /// Two packages have a crate of the name, under two directories: the first two
    /// found.
    Two(&'a Target, &'a Target),
    None,
}

/// The targets of the project at one side, in the order they are read (the package
/// at the root, then the members of its workspace in the order of their directories;
/// in a package, its library, its binaries, then its tests, benches and examples, each
/// by name), and what could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Targets {
    targets: Vec<Target>,
    unread: Vec<Unread>,
    top: Top,
    /// For each package, by its directory, the directories of the packages it depends
    /// on by a path (`[dependencies]` and `[target.<cfg>.dependencies]`, a `path` of
    /// its own or of the workspace's `[workspace.dependencies]`).
    depends: BTreeMap<Dir, BTreeSet<Dir>>,
}

/// The dependencies of a workspace that name a path, by their key, each with the
/// directory it names (from the root of the project).
type SharedPaths = BTreeMap<String, Dir>;

impl Targets {
    /// The targets of the project at `side`, read from the manifest at its root and
    /// those of the members of its workspace.
    pub(super) async fn of(state: &ProjectTree, side: Side) -> Self {
        let mut targets = Self::default();
        targets.read_from(state, side, &Dir::new()).await;
        targets
    }

    /// The targets of the project at `side` as the space `rust-item` reads them: those
    /// of [`of`](Self::of); and, when the root has no manifest, those of the manifest
    /// of each directory at the top of the tree (not one whose name starts with `.`),
    /// each read as the root's would be (its package and the members of its
    /// workspace), in the order of their names. A directory at the top that cannot be
    /// listed is kept as not read.
    pub(super) async fn of_project(state: &ProjectTree, side: Side) -> Self {
        let mut targets = Self::of(state, side).await;
        if targets.top == Top::Read {
            return targets;
        }
        let dirs = match subdirs(state, side, &Dir::new()).await {
            Ok(dirs) => dirs,
            Err(why) => {
                targets.unread.push(why);
                return targets;
            }
        };
        for name in dirs.into_iter().filter(|name| !name.starts_with('.')) {
            let base = vec![name];
            let unread = targets.unread.len();
            if targets.read_from(state, side, &base).await || targets.unread.len() > unread {
                targets.top = Top::Read;
            }
        }
        targets
    }

    /// Reads the manifest of `base` as the one of a root: its package and each member
    /// of its workspace, the members and `exclude` read from `base`. Whether there is a
    /// manifest there that was read.
    async fn read_from(&mut self, state: &ProjectTree, side: Side, base: &Dir) -> bool {
        let Some(root) = self.manifest(state, side, base).await else {
            return false;
        };
        let top = base.clone();
        let workspace = self.note(&top, table_field(&root, "", "workspace"));
        // The edition a member inherits with `edition.workspace = true`.
        let shared =
            workspace.and_then(|w| self.note(&top, table_field(w, "workspace", "package")));
        let inherited = shared
            .and_then(|p| self.note(&top, str_field(p, "workspace.package", "edition")))
            .map(str::to_owned);
        // The dependencies by path a member inherits with `workspace = true`.
        let paths: SharedPaths = workspace
            .and_then(|w| self.note(&top, table_field(w, "workspace", "dependencies")))
            .map(|deps| {
                deps.iter()
                    .filter_map(|(key, value)| {
                        let path = value.as_table()?.get("path")?.as_str()?;
                        Some((key.clone(), joined_dir(&top, path)?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.read_package(state, side, &root, &top, inherited.as_deref(), &paths)
            .await;
        let Some(workspace) = workspace else {
            return true;
        };
        let excluded: Vec<Dir> = self
            .workspace_texts(workspace, "exclude", &top)
            .iter()
            .map(|t| under_all(&top, &parts_of(t)))
            .collect();
        let listed = self.workspace_texts(workspace, "members", &top);
        let mut members: BTreeSet<Dir> = BTreeSet::new();
        for member in listed {
            let pattern = parts_of(&member);
            // A member written as a path is a member whatever `exclude` says; what a
            // pattern matches is not, under a path `exclude` names.
            if !pattern.iter().any(|part| is_pattern(part)) {
                members.insert(under_all(&top, &pattern));
                continue;
            }
            match self.expand(state, side, &top, &member).await {
                Ok(dirs) => members.extend(
                    dirs.into_iter()
                        .filter(|dir| !excluded.iter().any(|out| dir.starts_with(out))),
                ),
                Err(why) => self.unread.push(why),
            }
        }
        members.remove(&top);
        for dir in &members {
            if let Some(manifest) = self.manifest(state, side, dir).await {
                self.read_package(state, side, &manifest, dir, inherited.as_deref(), &paths)
                    .await;
            }
        }
        true
    }

    /// The crates of the project the package of `target` depends on by a path: the
    /// library of each package at a directory its manifest names so.
    pub(super) fn path_dependencies(&self, target: &Target) -> Vec<&Target> {
        let Some(dirs) = self.depends.get(&target.package) else {
            return Vec::new();
        };
        self.targets
            .iter()
            .filter(|other| other.kind == Kind::Lib && dirs.contains(&other.package))
            .collect()
    }

    /// Keeps the directories of the packages the manifest `manifest` of `dir` depends
    /// on by a path: of its `[dependencies]` and each `[target.<cfg>.dependencies]`, a
    /// table with a `path`, or with `workspace = true` for a key `shared` names. Any
    /// other dependency names no package of the project, and is passed over.
    fn keep_dependencies(&mut self, manifest: &toml::Table, dir: &Dir, shared: &SharedPaths) {
        let mut tables: Vec<&toml::Table> = Vec::new();
        tables.extend(manifest.get("dependencies").and_then(Value::as_table));
        if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
            tables.extend(
                targets
                    .values()
                    .filter_map(|of| of.get("dependencies")?.as_table()),
            );
        }
        let mut found = BTreeSet::new();
        for (key, value) in tables.into_iter().flatten() {
            let Some(dep) = value.as_table() else {
                continue;
            };
            if let Some(path) = dep.get("path").and_then(Value::as_str) {
                found.extend(joined_dir(dir, path));
            } else if dep.get("workspace").and_then(Value::as_bool) == Some(true) {
                found.extend(shared.get(key).cloned());
            }
        }
        if !found.is_empty() {
            self.depends.entry(dir.clone()).or_default().extend(found);
        }
    }

    /// Every target, in the order they were read.
    pub(super) fn all(&self) -> &[Target] {
        &self.targets
    }

    /// What could not be read, each with why.
    pub(super) fn unread(&self) -> &[Unread] {
        &self.unread
    }

    /// Whether the project has a manifest at its root.
    pub(super) fn top(&self) -> Top {
        self.top
    }

    /// The crate of the name `name`: the library of the name when a package has one,
    /// else the binary; two when two packages have one of the name under two
    /// directories (one directory twice is one crate); none when no package has one.
    /// The one place a name of a crate is looked up, and the order of the lookup.
    pub(super) fn crate_named(&self, name: &str) -> CrateOf<'_> {
        for kind in [Kind::Lib, Kind::Bin] {
            let mut of = self
                .targets
                .iter()
                .filter(|target| target.kind == kind && target.name == name);
            let Some(first) = of.next() else {
                continue;
            };
            let dir = first.root_dir();
            return match of.find(|other| other.root_dir() != dir) {
                Some(second) => CrateOf::Two(first, second),
                None => CrateOf::One(first),
            };
        }
        CrateOf::None
    }

    /// The manifest of the directory `dir`, parsed, or none (with why, kept, when it
    /// is there and cannot be read). The one at the root, there, is the [`Top`].
    async fn manifest(
        &mut self,
        state: &ProjectTree,
        side: Side,
        dir: &Dir,
    ) -> Option<toml::Table> {
        let mut at = dir.clone();
        at.push(MANIFEST.to_owned());
        let shown = at.join("/");
        let unread = |why: String| Unread::Manifest {
            path: shown.clone(),
            why,
        };
        let Ok(rel) = shown.parse::<RelPath>() else {
            self.unread
                .push(unread(format!("{shown} is no path of the project")));
            return None;
        };
        let read = file::read_within(state, side, &rel, MAX_SETTINGS_BYTES).await;
        if matches!(read, Ok(Bounded::NoFile)) {
            return None;
        }
        if dir.is_empty() {
            self.top = Top::Read;
        }
        let bytes = match read {
            Ok(Bounded::Bytes(bytes)) => bytes,
            Ok(Bounded::NoFile) => return None,
            Ok(Bounded::Outside) => {
                self.unread.push(unread(format!(
                    "{shown} leads outside the root of the project, or is broken"
                )));
                return None;
            }
            Ok(Bounded::TooLarge) => {
                self.unread.push(unread(format!(
                    "{shown} is larger than {MAX_SETTINGS_BYTES} bytes"
                )));
                return None;
            }
            Ok(Bounded::Untold(why)) => {
                self.unread
                    .push(unread(format!("{shown} cannot be read: {why}")));
                return None;
            }
            Err(e) => {
                self.unread
                    .push(unread(format!("{shown} cannot be read: {e}")));
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
                self.unread.push(unread(why));
                None
            }
        }
    }

    /// Whether the package the manifest `manifest` of `dir` declares has a library, as
    /// cargo tells it: its `[lib]`; else, unless `autolib = false`, a file
    /// `src/lib.rs`.
    async fn has_library(
        &mut self,
        state: &ProjectTree,
        side: Side,
        manifest: &toml::Table,
        dir: &Dir,
    ) -> bool {
        if manifest.get("lib").is_some() {
            return true;
        }
        if self.package_flag(manifest, dir, "autolib") == Some(false) {
            return false;
        }
        self.files_under(state, side, &under(dir, &[SOURCE_DIR]))
            .await
            .contains("lib.rs")
    }

    /// The targets of the package the manifest `manifest` of `dir` declares, if it
    /// declares one: its library ([`Self::has_library`]), its binaries, those its
    /// layout holds ([`Self::layout_bins`]) when cargo finds them, with each declared
    /// one (`[[bin]]`) in the place of one of its name, and its tests, benches and
    /// examples, found the same way. `inherited` is the edition of the workspace, for a
    /// package that inherits it, and `shared` its dependencies by a path.
    async fn read_package(
        &mut self,
        state: &ProjectTree,
        side: Side,
        manifest: &toml::Table,
        dir: &Dir,
        inherited: Option<&str>,
        shared: &SharedPaths,
    ) {
        let Some(name) = self.package_name(manifest, dir) else {
            return;
        };
        self.keep_dependencies(manifest, dir, shared);
        let library = self.has_library(state, side, manifest, dir).await;
        // Cargo finds targets by the layout unless told not to; of the edition 2015, a
        // package that declares one of a kind finds none of it unless told to.
        let edition = self.edition(manifest, dir, inherited);
        let finds = |flag: Option<bool>, declared: &[(String, Option<String>)]| {
            flag.unwrap_or(edition.unwrap_or("2015") != "2015" || declared.is_empty())
        };
        let declared_bins = self.declared(manifest, dir, "bin");
        let layout = self.layout_bins(state, side, dir, name).await;
        let autobins = self.package_flag(manifest, dir, "autobins");
        let mut bins: BTreeMap<String, String> = match finds(autobins, &declared_bins) {
            true => layout.clone(),
            false => BTreeMap::new(),
        };
        for (bin_name, path) in declared_bins {
            let path = path
                .or_else(|| layout.get(&bin_name).cloned())
                .unwrap_or_else(|| match bin_name == name {
                    true => "src/main.rs".to_owned(),
                    false => format!("src/bin/{bin_name}.rs"),
                });
            bins.insert(bin_name, path);
        }
        let mut others = Vec::new();
        for (kind, table, found_in, flag) in FOUND_KINDS {
            let of_kind = self.declared(manifest, dir, table);
            let flag = self.package_flag(manifest, dir, flag);
            let finding = finds(flag, &of_kind);
            // The directory is listed only when a target is found in it, or one declared
            // with no path is looked for there: one not needed that cannot be listed is
            // no part of the project left unread.
            let layout = match finding || of_kind.iter().any(|(_, path)| path.is_none()) {
                true => self.layout_of(state, side, dir, found_in).await,
                false => BTreeMap::new(),
            };
            let mut found: BTreeMap<String, String> = match finding {
                true => layout.clone(),
                false => BTreeMap::new(),
            };
            for (target, path) in of_kind {
                let path = path
                    .or_else(|| layout.get(&target).cloned())
                    .unwrap_or_else(|| format!("{found_in}/{target}.rs"));
                found.insert(target, path);
            }
            others.extend(found.into_iter().map(|(name, path)| (*kind, name, path)));
        }
        self.package(manifest, dir, name, library, bins, others);
    }

    /// Keeps a field of the manifest of `dir` that is there in a shape not read, as
    /// [`Unread::Manifest`] with why; the value of one that is read; none for one that
    /// is absent or kept. The one place a field that is absent is told from one that is
    /// wrong is [`Field`]'s readers, and what they say is taken here.
    fn note<T>(&mut self, dir: &Dir, field: Field<T>) -> Option<T> {
        match field {
            Field::Is(value) => Some(value),
            Field::Absent => None,
            Field::Wrong(why) => {
                let path = manifest_path(dir);
                self.unread.push(Unread::Manifest {
                    why: format!("{path}: {why}, so it is not read as written"),
                    path,
                });
                None
            }
        }
    }

    /// The edition of the package of the manifest of `dir`: the string it writes, the
    /// `inherited` one of the workspace when it writes `{ workspace = true }`, none when
    /// it writes none; any other shape is kept as not read, and read as none.
    fn edition<'a>(
        &mut self,
        manifest: &'a toml::Table,
        dir: &Dir,
        inherited: Option<&'a str>,
    ) -> Option<&'a str> {
        let Field::Is(package) = table_field(manifest, "", "package") else {
            return None;
        };
        let read = field(
            package,
            "package",
            "edition",
            "a string or `{ workspace = true }`",
            Edition::of,
        );
        match self.note(dir, read)? {
            Edition::Own(edition) => Some(edition),
            Edition::Inherited => inherited,
        }
    }

    /// The flag `flag` of the `[package]` of the manifest of `dir`, when it writes one.
    fn package_flag(&mut self, manifest: &toml::Table, dir: &Dir, flag: &str) -> Option<bool> {
        let Field::Is(package) = table_field(manifest, "", "package") else {
            return None;
        };
        self.note(dir, bool_field(package, "package", flag))
    }

    /// The name of the package the manifest of `dir` declares, if it declares one: none
    /// when it has no `[package]` or no name there, and none, with why kept, when either
    /// is there and is not read.
    fn package_name<'a>(&mut self, manifest: &'a toml::Table, dir: &Dir) -> Option<&'a str> {
        let package = self.note(dir, table_field(manifest, "", "package"))?;
        self.note(dir, str_field(package, "package", "name"))
    }

    /// The targets of a kind the manifest of `dir` declares (`[[bin]]`, `[[test]]`), each
    /// by its name, with its path when it writes one. A table that is no array of tables,
    /// a target with a name or a path that is no string, are kept as not read; a target
    /// with no name is none, and one with no path has none.
    fn declared(
        &mut self,
        manifest: &toml::Table,
        dir: &Dir,
        table: &str,
    ) -> Vec<(String, Option<String>)> {
        let Some(items) = self.note(dir, array_field(manifest, "", table)) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for item in items {
            let label = format!("an item of `{table}`");
            let Some(target) = self.note(dir, value_as(item, &label, "a table", Value::as_table))
            else {
                continue;
            };
            let Some(name) = self.note(dir, str_field(target, table, "name")) else {
                continue;
            };
            let path = self.note(dir, str_field(target, table, "path"));
            found.push((name.to_owned(), path.map(str::to_owned)));
        }
        found
    }

    /// The strings of the array `key` of the `[workspace]` table of the manifest of
    /// `dir`: none when the key is absent; when it is there and is no array, or an item
    /// is no string, that is kept as not read.
    fn workspace_texts(&mut self, workspace: &toml::Table, key: &str, dir: &Dir) -> Vec<String> {
        let Some(items) = self.note(dir, array_field(workspace, "workspace", key)) else {
            return Vec::new();
        };
        let mut texts = Vec::new();
        let label = format!("an item of `workspace.{key}`");
        for item in items {
            let text = value_as(item, &label, "a string", Value::as_str);
            texts.extend(self.note(dir, text).map(str::to_owned));
        }
        texts
    }

    /// The binaries the layout of the package `name` at `dir` holds, by name, with
    /// their paths: `src/main.rs` by the name of the package, and each
    /// `src/bin/<name>.rs` and `src/bin/<name>/main.rs` by its name.
    async fn layout_bins(
        &mut self,
        state: &ProjectTree,
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
        found.extend(self.layout_of(state, side, dir, "src/bin").await);
        found
    }

    /// The targets the directory `found_in` of the package at `dir` holds, by name,
    /// with their paths: each `<found_in>/<name>.rs` and `<found_in>/<name>/main.rs`,
    /// the first when both are.
    async fn layout_of(
        &mut self,
        state: &ProjectTree,
        side: Side,
        dir: &Dir,
        found_in: &str,
    ) -> BTreeMap<String, String> {
        let mut found = BTreeMap::new();
        let parts: Vec<&str> = found_in.split('/').collect();
        let at = under(dir, &parts);
        for file in self.files_under(state, side, &at).await {
            if let Some(stem) = file.strip_suffix(".rs") {
                found.insert(stem.to_owned(), format!("{found_in}/{file}"));
            }
        }
        for sub in self.dirs_under(state, side, &at).await {
            let inside = under(&at, &[sub.as_str()]);
            if self
                .files_under(state, side, &inside)
                .await
                .contains("main.rs")
            {
                found
                    .entry(sub.clone())
                    .or_insert_with(|| format!("{found_in}/{sub}/main.rs"));
            }
        }
        found
    }

    /// The names of the files under `dir`: none when it is not there, and none, with
    /// why kept as not read, when it cannot be listed.
    async fn files_under(
        &mut self,
        state: &ProjectTree,
        side: Side,
        dir: &Dir,
    ) -> BTreeSet<String> {
        match subfiles(state, side, dir).await {
            Ok(files) => files,
            Err(why) => {
                self.unread.push(why);
                BTreeSet::new()
            }
        }
    }

    /// The names of the directories under `dir`, as [`Self::files_under`] lists files.
    async fn dirs_under(&mut self, state: &ProjectTree, side: Side, dir: &Dir) -> Vec<String> {
        match subdirs(state, side, dir).await {
            Ok(dirs) => dirs,
            Err(why) => {
                self.unread.push(why);
                Vec::new()
            }
        }
    }

    /// The targets of the package the manifest `manifest` of `dir` declares: its
    /// library when it has one (`library`), its binaries (`bins`, by name, with their
    /// paths), and its other targets (`others`, each of its kind, by name, with its
    /// path). A target whose root leads out of the project or is no path of it is not
    /// read, and is kept as [`Unread::Manifest`] of the package, with why.
    fn package(
        &mut self,
        manifest: &toml::Table,
        dir: &Dir,
        name: &str,
        library: bool,
        bins: BTreeMap<String, String>,
        others: Vec<(Kind, String, String)>,
    ) {
        let lib = self.note(dir, table_field(manifest, "", "lib"));
        let lib_name = lib
            .and_then(|l| self.note(dir, str_field(l, "lib", "name")))
            .map_or_else(|| name.replace('-', "_"), str::to_owned);
        let lib_path = lib
            .and_then(|l| self.note(dir, str_field(l, "lib", "path")))
            .map_or_else(|| "src/lib.rs".to_owned(), str::to_owned);
        let mut add = |kind: Kind, name: String, path: &str| match root_of(dir, path) {
            Ok(root) => self.targets.push(Target {
                kind,
                name,
                package: dir.clone(),
                root,
            }),
            Err(why) => {
                let mut manifest = dir.clone();
                manifest.push(MANIFEST.to_owned());
                self.unread.push(Unread::Manifest {
                    path: manifest.join("/"),
                    why: format!(
                        "the root of the target `{name}` of the package at {} cannot be read: {why}",
                        shown(dir)
                    ),
                });
            }
        };
        if library {
            add(Kind::Lib, lib_name, &lib_path);
        }
        for (bin_name, path) in bins {
            add(Kind::Bin, bin_name.replace('-', "_"), &path);
        }
        for (kind, target, path) in others {
            add(kind, target.replace('-', "_"), &path);
        }
    }

    /// The directories a member of the workspace names: itself when it is a path,
    /// and each directory its pattern matches that holds a manifest when it is one.
    async fn expand(
        &mut self,
        state: &ProjectTree,
        side: Side,
        base: &Dir,
        member: &str,
    ) -> Result<Vec<Dir>, Unread> {
        let pattern = parts_of(member);
        let unread = |why: String| Unread::Member {
            pattern: member.to_owned(),
            why,
        };
        if pattern.iter().any(|part| part == "**") {
            return Err(unread(format!(
                "the member `{member}` of the workspace holds `**`, which is not read"
            )));
        }
        if pattern.iter().any(|part| part.contains('[')) {
            return Err(unread(format!(
                "the member `{member}` of the workspace holds `[`, which is not read"
            )));
        }
        if !pattern.iter().any(|part| is_pattern(part)) {
            return Ok(vec![under_all(base, &pattern)]);
        }
        let mut dirs: Vec<Dir> = vec![base.clone()];
        for part in &pattern {
            let mut next = Vec::new();
            for dir in dirs {
                if !is_pattern(part) {
                    let mut under = dir;
                    under.push(part.clone());
                    next.push(under);
                    continue;
                }
                for entry in subdirs(state, side, &dir).await? {
                    if matches(part, &entry) {
                        next.push(under(&dir, &[entry.as_str()]));
                    }
                }
                // A directory reached through a link is not walked: what it holds
                // cannot be told, and is said.
                for link in entries(state, side, &dir, EntryKind::Link).await? {
                    if matches(part, &link) {
                        self.unread.push(unread(format!(
                            "the member {} of the workspace is a symbolic link, which is not followed",
                            under(&dir, &[link.as_str()]).join("/")
                        )));
                    }
                }
            }
            dirs = next;
        }
        let mut found = Vec::new();
        for dir in dirs {
            if subfiles(state, side, &dir).await?.contains(MANIFEST) {
                found.push(dir);
            }
        }
        Ok(found)
    }
}

/// A field of a manifest as it is read: absent, there in the shape asked for, or there
/// in another (with why, the key and the TOML type found). The one reader of a field:
/// whether a field is absent or wrong is told here and nowhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Field<T> {
    Absent,
    Wrong(String),
    Is(T),
}

/// What a package writes as its edition.
enum Edition<'a> {
    /// A string.
    Own(&'a str),
    /// `{ workspace = true }`: the edition of the workspace.
    Inherited,
}

impl<'a> Edition<'a> {
    /// The edition `value` writes; none for any other shape.
    fn of(value: &'a Value) -> Option<Self> {
        match value {
            Value::String(edition) => Some(Self::Own(edition)),
            Value::Table(table) if table.get("workspace") == Some(&Value::Boolean(true)) => {
                Some(Self::Inherited)
            }
            _ => None,
        }
    }
}

/// `value`, called `label` in a reason, read by `read` as what `expected` says.
fn value_as<'a, T>(
    value: &'a Value,
    label: &str,
    expected: &str,
    read: fn(&'a Value) -> Option<T>,
) -> Field<T> {
    match read(value) {
        Some(read) => Field::Is(read),
        None => Field::Wrong(format!(
            "{label} is {} where {expected} is expected",
            value.type_str()
        )),
    }
}

/// The field `key` of `table`, which stands under the key `parent` of the manifest
/// (none: at its top).
fn field<'a, T>(
    table: &'a toml::Table,
    parent: &str,
    key: &str,
    expected: &str,
    read: fn(&'a Value) -> Option<T>,
) -> Field<T> {
    let label = match parent.is_empty() {
        true => format!("`{key}`"),
        false => format!("`{parent}.{key}`"),
    };
    match table.get(key) {
        None => Field::Absent,
        Some(value) => value_as(value, &label, expected, read),
    }
}

fn str_field<'a>(table: &'a toml::Table, parent: &str, key: &str) -> Field<&'a str> {
    field(table, parent, key, "a string", Value::as_str)
}

fn bool_field(table: &toml::Table, parent: &str, key: &str) -> Field<bool> {
    field(table, parent, key, "a boolean", Value::as_bool)
}

fn table_field<'a>(table: &'a toml::Table, parent: &str, key: &str) -> Field<&'a toml::Table> {
    field(table, parent, key, "a table", Value::as_table)
}

fn array_field<'a>(table: &'a toml::Table, parent: &str, key: &str) -> Field<&'a Vec<Value>> {
    field(table, parent, key, "an array", Value::as_array)
}

/// The manifest of the directory `dir`, as a path of the project.
fn manifest_path(dir: &Dir) -> String {
    under(dir, &[MANIFEST]).join("/")
}

/// The names of the directories under `dir`, as the state lists them.
async fn subdirs(state: &ProjectTree, side: Side, dir: &Dir) -> Result<Vec<String>, Unread> {
    entries(state, side, dir, EntryKind::Dir).await
}

/// The names of the files under `dir`, as the state lists them.
async fn subfiles(state: &ProjectTree, side: Side, dir: &Dir) -> Result<BTreeSet<String>, Unread> {
    Ok(entries(state, side, dir, EntryKind::File)
        .await?
        .into_iter()
        .collect())
}

/// The names of the entries of `kind` under `dir`; none when it is no directory.
async fn entries(
    state: &ProjectTree,
    side: Side,
    dir: &Dir,
    kind: EntryKind,
) -> Result<Vec<String>, Unread> {
    let unread = |why: String| Unread::Dir {
        dir: shown(dir),
        why,
    };
    let at =
        match dir.is_empty() {
            true => None,
            false => Some(dir.join("/").parse::<RelPath>().map_err(|e| {
                unread(format!("{} is no path of the project: {e}", dir.join("/")))
            })?),
        };
    match state.list(side, at.as_ref()).await {
        Ok(DirListing::All(entries)) => Ok(entries
            .named
            .into_iter()
            .filter(|(_, of)| *of == kind)
            .filter_map(|(name, _)| name.as_str().rsplit('/').next().map(str::to_owned))
            .collect()),
        Ok(DirListing::Cut { why, .. }) => Err(unread(format!(
            "the directory {} of the workspace is not listed whole: {why}",
            shown(dir)
        ))),
        // Not there: nothing; anything else that is no directory cannot be told.
        Ok(DirListing::NotADir(NotADir::Absent | NotADir::File)) => Ok(Vec::new()),
        Ok(DirListing::NotADir(NotADir::Elsewhere { .. })) => Err(unread(format!(
            "{} is reached through a symbolic link, which is not followed",
            shown(dir)
        ))),
        Ok(DirListing::NotADir(NotADir::Untold(why))) => {
            Err(unread(format!("{} cannot be told: {why}", shown(dir))))
        }
        Err(e) => Err(unread(format!(
            "the directory {} cannot be listed: {e}",
            shown(dir)
        ))),
    }
}

/// `dir` with `parts` under it.
fn under(dir: &Dir, parts: &[&str]) -> Dir {
    let mut at = dir.clone();
    at.extend(parts.iter().map(|part| (*part).to_owned()));
    at
}

/// The root file `path` of a target of the package at `dir`, with `.` and `..` read;
/// why when it leads out of the project or is no path of it.
fn root_of(dir: &Dir, path: &str) -> Result<RelPath, String> {
    let mut at = dir.clone();
    let parts = parts_of(path);
    let (file, above) = parts
        .split_last()
        .ok_or_else(|| format!("`{path}` is no file"))?;
    for part in above {
        match part.as_str() {
            ".." => {
                if at.pop().is_none() {
                    return Err(format!("`{path}` leads out of the root of the project"));
                }
            }
            part => at.push(part.to_owned()),
        }
    }
    at.push(file.clone());
    at.join("/")
        .parse()
        .map_err(|e| format!("`{path}` is no path of the project: {e}"))
}

/// The parts of a path as a manifest writes it: `/` and `\` between them, the empty
/// and `.` ones left out.
fn parts_of(text: &str) -> Dir {
    text.split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_owned)
        .collect()
}

/// The directory `parts` names under `base`.
fn under_all(base: &Dir, parts: &Dir) -> Dir {
    base.iter().chain(parts).cloned().collect()
}

/// The directory the relative path `path` (a `path` of a dependency) names from
/// `dir`, `..` read; none when it climbs out of the project or is absolute.
fn joined_dir(dir: &Dir, path: &str) -> Option<Dir> {
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return None;
    }
    let mut at = dir.clone();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                at.pop()?;
            }
            part => at.push(part.to_owned()),
        }
    }
    Some(at)
}

/// Whether a part of a member is a pattern.
fn is_pattern(part: &str) -> bool {
    part.contains(['*', '?', '['])
}

/// Whether the name `name` matches the pattern `part`: `*` any run of characters,
/// `?` any one, every other character itself. A member with `[` is not read
/// (`Targets::expand` tells it so), and never comes here.
pub(super) fn matches(part: &str, name: &str) -> bool {
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
pub(super) fn shown(dir: &Dir) -> String {
    match dir.is_empty() {
        true => "the root".to_owned(),
        false => dir.join("/"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    async fn write(dir: &Path, at: &str, text: &str) {
        let full = dir.join(at);
        if let Some(parent) = full.parent() {
            // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
        tokio::fs::write(full, text).await.unwrap();
    }

    async fn targets_at(root: &Path) -> Targets {
        let tree = ProjectTree::files(crate::shelf::state::files::FileStates::new(
            root.to_path_buf(),
        ));
        Targets::of(&tree, Side::After).await
    }

    /// Each target as `(kind, name, package, root)`.
    fn table(targets: &Targets) -> Vec<(Kind, String, String, String)> {
        targets
            .all()
            .iter()
            .map(|t| {
                (
                    t.kind,
                    t.name.clone(),
                    t.package.join("/"),
                    t.root.as_str().to_owned(),
                )
            })
            .collect()
    }

    fn row(kind: Kind, name: &str, package: &str, root: &str) -> (Kind, String, String, String) {
        (kind, name.to_owned(), package.to_owned(), root.to_owned())
    }

    #[tokio::test]
    async fn the_targets_of_a_workspace_are_its_libraries_binaries_and_tests() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[workspace]\nmembers = [\"crates/*\"]\n[package]\nname = \"top-level\"\nedition = \"2021\"\n",
        )
        .await;
        write(root, "src/lib.rs", "").await;
        write(root, "tests/plain.rs", "").await;
        write(root, "tests/cli/main.rs", "").await;
        write(root, "tests/cli/common.rs", "").await;
        write(root, "tests/common/mod.rs", "").await;
        write(
            root,
            "crates/named/Cargo.toml",
            "[package]\nname = \"named\"\nedition = \"2021\"\n[lib]\nname = \"other\"\npath = \"code/top.rs\"\n[[test]]\nname = \"declared\"\npath = \"checks/declared.rs\"\n[[bench]]\nname = \"speed\"\n",
        )
        .await;
        write(root, "crates/named/benches/speed.rs", "").await;
        write(root, "crates/named/examples/demo.rs", "").await;
        write(root, "crates/named/tests/found.rs", "").await;
        write(
            root,
            "crates/off/Cargo.toml",
            "[package]\nname = \"off-tests\"\nautotests = false\n",
        )
        .await;
        write(root, "crates/off/src/lib.rs", "").await;
        write(root, "crates/off/tests/skipped.rs", "").await;
        let targets = targets_at(root).await;
        assert_eq!(
            table(&targets),
            vec![
                // The package at the root, its name with `-` as `_`; `tests/x/main.rs`
                // by its directory; `tests/common/mod.rs` is no target.
                row(Kind::Lib, "top_level", "", "src/lib.rs"),
                row(Kind::Test, "cli", "", "tests/cli/main.rs"),
                row(Kind::Test, "plain", "", "tests/plain.rs"),
                // `[lib] name` and path, a `[[test]]` beside those found, a `[[bench]]`
                // with no path at the one found.
                row(
                    Kind::Lib,
                    "other",
                    "crates/named",
                    "crates/named/code/top.rs"
                ),
                row(
                    Kind::Test,
                    "declared",
                    "crates/named",
                    "crates/named/checks/declared.rs"
                ),
                row(
                    Kind::Test,
                    "found",
                    "crates/named",
                    "crates/named/tests/found.rs"
                ),
                row(
                    Kind::Bench,
                    "speed",
                    "crates/named",
                    "crates/named/benches/speed.rs"
                ),
                row(
                    Kind::Example,
                    "demo",
                    "crates/named",
                    "crates/named/examples/demo.rs"
                ),
                // `autotests = false`: no test is found.
                row(
                    Kind::Lib,
                    "off_tests",
                    "crates/off",
                    "crates/off/src/lib.rs"
                ),
            ]
        );
        assert_eq!(targets.unread(), &[]);
        assert_eq!(targets.top(), Top::Read);
    }

    #[tokio::test]
    async fn a_name_is_the_library_before_the_binary_and_two_packages_of_one_name_are_two() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[workspace]\nmembers = [\"a\", \"b\", \"c\", \"d\"]\n",
        )
        .await;
        // Two packages with one binary name.
        for at in ["a", "b"] {
            write(
                root,
                &format!("{at}/Cargo.toml"),
                &format!(
                    "[package]\nname = \"{at}\"\n[[bin]]\nname = \"tool\"\npath = \"main.rs\"\n"
                ),
            )
            .await;
            write(root, &format!("{at}/main.rs"), "fn main() {}\n").await;
        }
        // A library and a binary of one name, in two packages.
        write(root, "c/Cargo.toml", "[package]\nname = \"both\"\n").await;
        write(root, "c/src/lib.rs", "").await;
        write(
            root,
            "d/Cargo.toml",
            "[package]\nname = \"d\"\n[[bin]]\nname = \"both\"\npath = \"src/main.rs\"\n",
        )
        .await;
        write(root, "d/src/main.rs", "fn main() {}\n").await;
        let targets = targets_at(root).await;
        match targets.crate_named("tool") {
            CrateOf::Two(one, two) => {
                assert_eq!(one.root.as_str(), "a/main.rs");
                assert_eq!(two.root.as_str(), "b/main.rs");
            }
            other => panic!("{other:?}"),
        }
        match targets.crate_named("both") {
            CrateOf::One(lib) => {
                assert_eq!(lib.kind, Kind::Lib);
                assert_eq!(lib.root.as_str(), "c/src/lib.rs");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(targets.crate_named("none"), CrateOf::None);
        // Two packages of one library name.
        write(root, "a/src/lib.rs", "").await;
        write(root, "a/Cargo.toml", "[package]\nname = \"same\"\n").await;
        write(root, "b/src/lib.rs", "").await;
        write(root, "b/Cargo.toml", "[package]\nname = \"same\"\n").await;
        let targets = targets_at(root).await;
        assert!(matches!(targets.crate_named("same"), CrateOf::Two(..)));
    }

    #[tokio::test]
    async fn a_member_written_as_a_pattern_not_read_is_unread_and_no_root_manifest_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, MANIFEST, "[workspace]\nmembers = [\"c/**\"]\n").await;
        let targets = targets_at(root).await;
        assert!(
            matches!(targets.unread(), [Unread::Member { pattern, why }] if pattern == "c/**" && why.contains("`**`")),
            "{:?}",
            targets.unread()
        );
        let bare = tempfile::tempdir().unwrap();
        let targets = targets_at(bare.path()).await;
        assert_eq!(targets.top(), Top::Absent);
        assert!(targets.all().is_empty() && targets.unread().is_empty());
    }

    /// At the level of `Targets::of`: the unread are listed, which is what membership
    /// turns into cannot tell.
    #[tokio::test]
    async fn a_target_root_out_of_the_project_is_unread_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[package]\nname = \"demo\"\n[[bin]]\nname = \"far\"\npath = \"../../x.rs\"\n",
        )
        .await;
        write(root, "src/lib.rs", "").await;
        let targets = targets_at(root).await;
        assert_eq!(
            table(&targets),
            vec![row(Kind::Lib, "demo", "", "src/lib.rs")]
        );
        assert!(
            matches!(targets.unread(), [Unread::Manifest { path, why }]
                if path == MANIFEST && why.contains("`far`") && why.contains("leads out of the root")),
            "{:?}",
            targets.unread()
        );
    }

    /// The unread whys of the project at `root` whose manifest is `manifest` and whose
    /// library is at `src/lib.rs`, with the targets it reads.
    async fn unread_of(manifest: &str) -> (Vec<(Kind, String)>, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, MANIFEST, manifest).await;
        write(root, "src/lib.rs", "").await;
        write(root, "src/bin/tool.rs", "").await;
        let targets = targets_at(root).await;
        (
            targets
                .all()
                .iter()
                .map(|t| (t.kind, t.name.clone()))
                .collect(),
            targets.unread().iter().map(ToString::to_string).collect(),
        )
    }

    #[tokio::test]
    async fn a_field_of_a_manifest_in_the_wrong_shape_is_unread_and_the_rest_is_read() {
        let pkg = "[package]\nname = \"demo\"\n";
        let lib = (Kind::Lib, "demo".to_owned());
        let tool = (Kind::Bin, "tool".to_owned());
        // `[[bin]]` that is no array of tables: unread, the layout still finds the bin.
        let (read, unread) = unread_of(&format!("bin = \"x\"\n{pkg}")).await;
        assert_eq!(read, [lib.clone(), tool.clone()]);
        assert!(
            unread.iter().any(|w| w.contains("`bin` is string")),
            "{unread:?}"
        );
        // A bin whose name is no string: that bin unread, the rest read.
        let (read, unread) =
            unread_of(&format!("{pkg}[[bin]]\nname = 1\npath = \"src/x.rs\"\n")).await;
        assert_eq!(read, [lib.clone(), tool.clone()]);
        assert!(
            unread.iter().any(|w| w.contains("`bin.name` is integer")),
            "{unread:?}"
        );
        // A name that is no string: the package is skipped, with why.
        let (read, unread) = unread_of("[package]\nname = 1\n").await;
        assert!(read.is_empty());
        assert!(
            unread
                .iter()
                .any(|w| w.contains("`package.name` is integer")),
            "{unread:?}"
        );
        // A flag that is no boolean: the library is found as if it were unwritten.
        let (read, unread) = unread_of(&format!("{pkg}autolib = \"no\"\n")).await;
        assert_eq!(read, [lib.clone(), tool.clone()]);
        assert!(
            unread
                .iter()
                .any(|w| w.contains("`package.autolib` is string")),
            "{unread:?}"
        );
        // A workspace that is no table: no workspace, the package at the root read.
        let (read, unread) = unread_of(&format!("workspace = 1\n{pkg}")).await;
        assert_eq!(read, [lib.clone(), tool.clone()]);
        assert!(
            unread.iter().any(|w| w.contains("`workspace` is integer")),
            "{unread:?}"
        );
        // A `[lib]` name that is no string.
        let (read, unread) = unread_of(&format!("{pkg}[lib]\nname = 2\n")).await;
        assert_eq!(read, [lib, tool]);
        assert!(
            unread.iter().any(|w| w.contains("`lib.name` is integer")),
            "{unread:?}"
        );
        // An edition that is no string and no `{ workspace = true }`.
        let (read, unread) = unread_of("[package]\nname = \"demo\"\nedition = 2021\n").await;
        assert_eq!(read.len(), 2);
        assert!(
            unread
                .iter()
                .any(|w| w.contains("`package.edition` is integer")),
            "{unread:?}"
        );
        // `edition.workspace = true` is inherited: nothing unread.
        let (_, unread) = unread_of("[package]\nname = \"demo\"\nedition.workspace = true\n").await;
        assert!(unread.is_empty(), "{unread:?}");
        // Nothing wrong, nothing unread.
        assert!(unread_of(pkg).await.1.is_empty());
    }

    #[tokio::test]
    async fn a_workspace_key_of_the_wrong_shape_is_unread_and_an_absent_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, MANIFEST, "[workspace]\nmembers = \"crates/*\"\n").await;
        let targets = targets_at(root).await;
        assert!(
            matches!(targets.unread(), [Unread::Manifest { why, .. }] if why.contains("`workspace.members`")),
            "{:?}",
            targets.unread()
        );
        // An item that is no string is unread, the others are read.
        write(root, MANIFEST, "[workspace]\nmembers = [\"a\", 1]\n").await;
        write(root, "a/Cargo.toml", "[package]\nname = \"a\"\n").await;
        write(root, "a/src/lib.rs", "").await;
        let targets = targets_at(root).await;
        assert_eq!(
            table(&targets),
            vec![row(Kind::Lib, "a", "a", "a/src/lib.rs")]
        );
        assert!(
            matches!(targets.unread(), [Unread::Manifest { why, .. }] if why.contains("an item of `workspace.members`")),
            "{:?}",
            targets.unread()
        );
        // Neither key present: nothing unread.
        write(root, MANIFEST, "[workspace]\n").await;
        assert!(targets_at(root).await.unread().is_empty());
    }
}
