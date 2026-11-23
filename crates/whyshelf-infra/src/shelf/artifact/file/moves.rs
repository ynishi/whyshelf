//! The search for where a gone file moved: the same content in the work tree, then
//! Git's renames (the rules on the module page, "Suggesting moves").
//!
//! The search is of the project as it is ([`Side::After`]): its walk of the work tree
//! and Git's history are read here directly, and what a path is (a file through no
//! link out of the root, named as written) is read through the state of the project,
//! by the same rule as a reading of the space.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use whyshelf_core::app::shelf::{AtSide, ByteBound, Side};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{ArtifactHash, ArtifactName, Comparison, Revision};
use whyshelf_core::domain::shelf::ledger::{
    FoundBy, GoneName, MoveCandidate, MoveNote, NameSuggestions, PinnedThere, Similarity,
};

use super::super::super::git::{RenamesSince, first_line};
use super::super::super::state::StateError;
use super::super::super::state::files::FileStates;
use super::super::super::state::paths::{Followed, Leave, follow, look_through, named_as_written};
use super::read::is_absent;
use super::{ArtifactError, FileSpace, ScanLimits};

/// What a path of the project is now, by the rule of a reading of the space, within
/// `most` bytes: a file (with its bytes), no file, or what cannot be read (counted
/// apart by a caller).
enum Path {
    /// A file, of this length.
    File(u64),
    NoFile,
    Unreadable,
}

/// The path at the side after, looked at through no link out of the root: nothing is
/// opened.
async fn path_now(state: &FileStates, path: &RelPath) -> Path {
    match look_through(state, Side::After, path).await {
        Ok(Followed::Reached {
            answer: AtSide::There(len),
            ..
        }) => Path::File(len),
        Ok(Followed::Reached { .. } | Followed::Left | Followed::Broken | Followed::TooMany) => {
            Path::NoFile
        }
        Err(StateError::Io { source, .. }) if is_absent(&source) => Path::NoFile,
        Err(_) => Path::Unreadable,
    }
}

/// The bytes of a file at the side after, at most `most`, read through no link out of
/// the root: none when it is no longer a file, and an error when it cannot be read.
async fn bytes_now(state: &FileStates, path: &RelPath, most: u64) -> Result<Option<Vec<u8>>, ()> {
    match follow(
        state,
        Side::After,
        path,
        Leave::Refused,
        ByteBound::at_most(most),
    )
    .await
    {
        Ok(Followed::Reached {
            answer: AtSide::There(bytes),
            ..
        }) => Ok(Some(bytes)),
        Ok(_) => Ok(None),
        Err(StateError::Io { source, .. }) if is_absent(&source) => Ok(None),
        Err(_) => Err(()),
    }
}

/// What one search for the same content passed over or stopped at.
#[derive(Debug, Default)]
struct Tally {
    read: usize,
    bytes: u64,
    unreadable: usize,
    too_large: usize,
    truncated: bool,
}

impl Tally {
    /// Its notes, each when it has something to say.
    fn notes(&self, limits: &ScanLimits) -> Vec<MoveNote> {
        let mut notes = Vec::new();
        if self.truncated {
            notes.push(MoveNote::ScanTruncated { read: self.read });
        }
        if self.unreadable > 0 {
            notes.push(MoveNote::UnreadableSkipped {
                count: self.unreadable,
            });
        }
        if self.too_large > 0 {
            notes.push(MoveNote::TooLargeSkipped {
                count: self.too_large,
                limit: limits.file_bytes,
            });
        }
        notes
    }
}

impl FileSpace {
    /// For each gone name, the files it may have moved to (the rules on the module
    /// page, "Suggesting moves"): the files there now with the content pinned, then
    /// the renames Git detects since each approval's revision; with a note of what
    /// could not be looked at. Every name asked about has an entry.
    pub async fn suggest(
        &self,
        gone: &[GoneName],
    ) -> Result<BTreeMap<ArtifactName, NameSuggestions>, ArtifactError> {
        let mut found: BTreeMap<ArtifactName, Vec<MoveCandidate>> = gone
            .iter()
            .map(|g| (g.name().clone(), Vec::new()))
            .collect();
        let mut notes: BTreeMap<ArtifactName, Vec<MoveNote>> = BTreeMap::new();

        // 1. The same content, in the hash the pin used.
        let mut wanted: BTreeMap<&Comparison, BTreeMap<&ArtifactHash, BTreeSet<&ArtifactName>>> =
            BTreeMap::new();
        for name in gone {
            for (comparison, hash) in name.pins().iter().filter_map(PinnedThere::content) {
                wanted
                    .entry(comparison)
                    .or_default()
                    .entry(hash)
                    .or_default()
                    .insert(name.name());
            }
        }
        if !wanted.is_empty() {
            // The comparisons, in the order of `wanted`: each file is hashed under
            // them all, a large one on a blocking thread, as a file may be as large
            // as the bound.
            let comparisons: std::sync::Arc<[Comparison]> =
                wanted.keys().map(|c| (*c).clone()).collect();
            let mut tally = Tally::default();
            let files = match self.git.work_tree_files(self.limits.listing_bytes).await? {
                Some(listing) => {
                    tally.truncated |= listing.cut;
                    listing.files
                }
                None => walk(&self.root, &self.limits, &mut tally).await,
            };
            for path in &files {
                let Some(bytes) = scan_read(&self.state, path, &self.limits, &mut tally).await
                else {
                    if tally.truncated {
                        break;
                    }
                    continue;
                };
                let asked = std::sync::Arc::clone(&comparisons);
                let Ok(hashes) = super::super::on_blocking(bytes.len(), move || {
                    asked
                        .iter()
                        .map(|comparison| comparison.fingerprint(&bytes))
                        .collect::<Vec<_>>()
                })
                .await
                else {
                    tally.unreadable += 1;
                    continue;
                };
                let mut matched: BTreeSet<&ArtifactName> = BTreeSet::new();
                for (by_hash, hash) in wanted.values().zip(&hashes) {
                    if let Some(names) = by_hash.get(hash) {
                        matched.extend(names.iter().copied());
                    }
                }
                if matched.is_empty() {
                    continue;
                }
                // The candidate is the name as written on disk, which a file system
                // that does not tell case apart may spell otherwise than the listing.
                let Some(at) = written_on_disk(&self.root, &self.state, path)
                    .await
                    .and_then(|at| ArtifactName::new(at.as_str()).ok())
                else {
                    continue;
                };
                for name in matched {
                    if let Some(candidates) = found.get_mut(name) {
                        candidates.push(MoveCandidate::new(at.clone(), FoundBy::SameContent, true));
                    }
                }
            }
            let scanned = tally.notes(&self.limits);
            for name in gone {
                if name.pins().iter().any(|pin| pin.content().is_some()) {
                    notes
                        .entry(name.name().clone())
                        .or_default()
                        .extend(scanned.iter().cloned());
                }
            }
        }
        // A name pinned by its presence only has no content to look for: say so, so
        // that no candidate is not read as nothing found.
        for name in gone {
            if !name.pins().iter().any(|pin| pin.content().is_some()) {
                notes
                    .entry(name.name().clone())
                    .or_default()
                    .push(MoveNote::NoContent);
            }
        }

        // 2. Git's rename detection, since the revision of each approval.
        let mut by_revision: BTreeMap<&Revision, BTreeMap<&ArtifactName, BTreeSet<String>>> =
            BTreeMap::new();
        for name in gone {
            let mut kept_one = false;
            for pin in name.pins().iter() {
                if let Some(revision) = pin.revision() {
                    kept_one = true;
                    // At the revision, the file had the name the pin wrote; a move
                    // declared since may have named it otherwise.
                    let olds = by_revision
                        .entry(revision)
                        .or_default()
                        .entry(name.name())
                        .or_default();
                    olds.insert(name.name().as_str().to_owned());
                    olds.insert(pin.written().name().as_str().to_owned());
                }
            }
            // Only when no pin of the name kept one: a pin that did is looked from.
            if !kept_one {
                notes
                    .entry(name.name().clone())
                    .or_default()
                    .push(MoveNote::NoRevision);
            }
        }
        let mut unknown: BTreeMap<&ArtifactName, BTreeSet<Revision>> = BTreeMap::new();
        for (revision, names) in by_revision {
            let found_since = match self
                .git
                .renames_since(revision, self.limits.listing_bytes)
                .await
            {
                Ok(found_since) => found_since,
                // A history that cannot be read stops this way of looking, not the
                // answer.
                Err(error) => {
                    // One line: what Git said is cut to its first line.
                    let detail = NonBlank::new(first_line(&error.to_string()))
                        .map_err(ArtifactError::Reason)?;
                    for name in names.keys() {
                        notes.entry((*name).clone()).or_default().push(
                            MoveNote::HistoryUnreadable {
                                detail: detail.clone(),
                            },
                        );
                    }
                    continue;
                }
            };
            match found_since {
                RenamesSince::Found(renames) => {
                    for rename in renames {
                        let present = match path_now(&self.state, &rename.to).await {
                            Path::File(_) => {
                                named_as_written(&self.state, Side::After, &rename.to).await
                            }
                            // A place that cannot be looked at is not said to have a
                            // file: `present` is what the adapter can say is there.
                            Path::NoFile | Path::Unreadable => false,
                        };
                        let Ok(to) = ArtifactName::new(rename.to.as_str()) else {
                            continue;
                        };
                        let similarity = rename.similarity.and_then(Similarity::new);
                        for (name, olds) in &names {
                            if !olds.contains(rename.from.as_str()) {
                                continue;
                            }
                            if let Some(candidates) = found.get_mut(*name) {
                                candidates.push(MoveCandidate::new(
                                    to.clone(),
                                    FoundBy::Renamed { similarity },
                                    present,
                                ));
                            }
                        }
                    }
                }
                RenamesSince::NotARepository => {
                    for name in names.keys() {
                        notes
                            .entry((*name).clone())
                            .or_default()
                            .push(MoveNote::NoRepository);
                    }
                }
                RenamesSince::UnknownRevision => {
                    for name in names.keys() {
                        unknown.entry(name).or_default().insert(revision.clone());
                    }
                }
            }
        }
        for (name, revisions) in unknown {
            notes
                .entry(name.clone())
                .or_default()
                .push(MoveNote::UnknownRevision { revisions });
        }

        Ok(found
            .into_iter()
            .map(|(name, candidates)| {
                let mut notes = notes.remove(&name).unwrap_or_default();
                notes.sort_by_key(MoveNote::kind);
                notes.dedup_by_key(|n| n.kind());
                (name, NameSuggestions { candidates, notes })
            })
            .collect())
    }
}

/// The files under `root`, outside a repository: every regular file and symbolic
/// link, by its path, in no directory or file whose name starts with `.`, never
/// walking into a link (`read_within` follows one only inside the root when it is
/// read), up to `limits.files` of them (more marks the tally truncated). A
/// directory or an entry that cannot be read is passed over and counted.
async fn walk(root: &std::path::Path, limits: &ScanLimits, tally: &mut Tally) -> Vec<RelPath> {
    let mut files = Vec::new();
    let mut dirs: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = dirs.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(e) if is_absent(&e) => continue,
            Err(_) => {
                tally.unreadable += 1;
                continue;
            }
        };
        let mut found = Vec::new();
        loop {
            match entries.next_entry().await {
                Ok(Some(entry)) => found.push(entry),
                Ok(None) => break,
                Err(_) => {
                    tally.unreadable += 1;
                    break;
                }
            }
        }
        found.sort_by_key(|entry| entry.file_name());
        for entry in found {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type().await else {
                tally.unreadable += 1;
                continue;
            };
            let at = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            if kind.is_dir() {
                dirs.push((entry.path(), at));
            // A link is listed and never walked into (no loop): reading it, `read_within`
            // follows it only inside the root.
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            } else if (kind.is_file() || kind.is_symlink())
                && let Ok(path) = at.parse::<RelPath>()
            {
                if files.len() == limits.files {
                    tally.truncated = true;
                    return files;
                }
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// The bytes of one file for the search for the same content, within `limits`:
/// none for no regular file, a link on the way out of the root, a file larger than
/// one reading may be, one that cannot be read (each of the last two counted), and
/// once a bound of the search is reached (the tally marked truncated).
async fn scan_read(
    state: &FileStates,
    path: &RelPath,
    limits: &ScanLimits,
    tally: &mut Tally,
) -> Option<Vec<u8>> {
    if tally.truncated {
        return None;
    }
    if tally.read == limits.files {
        tally.truncated = true;
        return None;
    }
    let len = match path_now(state, path).await {
        Path::File(len) => len,
        Path::NoFile => return None,
        Path::Unreadable => {
            tally.unreadable += 1;
            return None;
        }
    };
    if len > limits.file_bytes {
        tally.too_large += 1;
        return None;
    }
    if tally.bytes.saturating_add(len) > limits.total_bytes {
        tally.truncated = true;
        return None;
    }
    let bytes = match bytes_now(state, path, limits.file_bytes).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return None,
        Err(()) => {
            tally.unreadable += 1;
            return None;
        }
    };
    tally.read += 1;
    tally.bytes = tally
        .bytes
        .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    Some(bytes)
}

/// The path as its entries are written on disk, part by part: the entry named as
/// written, else the one entry that differs from it in ASCII case only (what a file
/// system that does not tell case apart opened), else the part as written. Kept only
/// when [`named_as_written`] holds it to name the file, so a candidate is always a
/// name `judge moved` takes; none otherwise.
async fn written_on_disk(
    root: &std::path::Path,
    state: &FileStates,
    path: &RelPath,
) -> Option<RelPath> {
    let mut dir = root.to_path_buf();
    let mut parts = Vec::new();
    for part in path.as_str().split('/') {
        let mut spelled = part.to_owned();
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            let mut others = Vec::new();
            let mut written = false;
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                if name == part {
                    written = true;
                    break;
                }
                if let Some(name) = name.to_str()
                    && name.eq_ignore_ascii_case(part)
                {
                    others.push(name.to_owned());
                }
            }
            if !written && let [only] = others.as_slice() {
                spelled.clone_from(only);
            }
        }
        dir.push(&spelled);
        parts.push(spelled);
    }
    let on_disk: RelPath = parts.join("/").parse().ok()?;
    named_as_written(state, Side::After, &on_disk)
        .await
        .then_some(on_disk)
}
