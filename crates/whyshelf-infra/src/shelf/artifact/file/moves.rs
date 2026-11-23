//! The search for where a gone file moved: the same content in the work tree, then
//! Git's renames (the rules on the module page, "Suggesting moves").

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{ArtifactHash, ArtifactName, Comparison, Revision};
use whyshelf_core::domain::shelf::ledger::{
    FoundBy, GoneName, MoveCandidate, MoveNote, NameSuggestions, PinnedThere, Similarity,
};

use super::super::super::git::{RenamesSince, first_line};
use super::read::{Place, is_absent, named_as_written, place_of};
use super::{ArtifactError, FileSpace, ScanLimits};

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
                let Some(bytes) = scan_read(&self.root, path, &self.limits, &mut tally).await
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
                let Some(at) = written_on_disk(&self.root, path)
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
                        let present = match place_of(&self.root, &rename.to).await {
                            Ok(Place::File { .. }) => {
                                named_as_written(&self.root, &rename.to).await
                            }
                            // A place that cannot be looked at is not said to have a
                            // file: `present` is what the adapter can say is there.
                            Ok(Place::NoFile | Place::Outside) | Err(_) => false,
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
/// walking into a link (`place_of` follows one only inside the root when it is
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
            // A link is listed and never walked into (no loop): reading it, `place_of`
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
/// none for no regular file, a link on the way, a file larger than one reading may
/// be, one that cannot be read (each of the last two counted), and once a bound of
/// the search is reached (the tally marked truncated).
async fn scan_read(
    root: &std::path::Path,
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
    let len = match place_of(root, path).await {
        Ok(Place::File { len }) => len,
        Ok(Place::NoFile | Place::Outside) => return None,
        Err(_) => {
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
    match tokio::fs::read(root.join(path.as_str())).await {
        Ok(bytes) => {
            tally.read += 1;
            tally.bytes = tally
                .bytes
                .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
            Some(bytes)
        }
        Err(e) if is_absent(&e) => None,
        Err(_) => {
            tally.unreadable += 1;
            None
        }
    }
}

/// The path as its entries are written on disk, part by part: the entry named as
/// written, else the one entry that differs from it in ASCII case only (what a file
/// system that does not tell case apart opened), else the part as written. Kept only
/// when [`named_as_written`] holds it to name the file, so a candidate is always a
/// name `judge moved` takes; none otherwise.
async fn written_on_disk(root: &std::path::Path, path: &RelPath) -> Option<RelPath> {
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
    named_as_written(root, &on_disk).await.then_some(on_disk)
}
