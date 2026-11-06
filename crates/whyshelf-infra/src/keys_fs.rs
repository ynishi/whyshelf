//! The keys that pin private upstreams: [`KeySource`] over the person's own data.
//!
//! A key is a secret of the person, not of the project, so it is kept with the
//! person's data and never in a store (a store can be remade or shared, and a key
//! that went with it would be lost or published). Where, and what each file is:
//!
//! ```text
//! <data dir>/keys/              the data dir is `UserDirs::data_dir`:
//!                               $WHYSHELF_DATA_DIR, $XDG_DATA_HOME/whyshelf, or
//!                               $HOME/.local/share/whyshelf; mode 0700
//!   <kid>.key                   one key: 64 lowercase hex digits and a line break; mode 0600
//!   current                     the kid new pins are made under, and a line break
//! ```
//!
//! # Reading never makes a key
//!
//! [`KeySource::keys`] only reads. A person with no `keys` directory (or none in
//! it) has an empty keyring, which is a known thing: such a person can verify no
//! keyed pin and make none, and the gate says so. A key comes into being only when
//! [`FsKeys::ensure_key`] or [`FsKeys::rotate`] is called, which is what the
//! command that sets a project up calls, on purpose. A read that made a key would
//! make one on every machine that ever looked, including a CI run, where it would
//! be lost with the run.
//!
//! | state of the directory | [`KeySource::keys`] says |
//! |---|---|
//! | the environment has no data dir | [`Knowable::Unknown`] |
//! | no `keys` directory, or no key in it | a known empty keyring |
//! | keys and a `current` that names one of them | the keyring, that key current, the others kept |
//! | a key file that is not 64 hex digits, a `current` that is no kid or names no key, or keys and no `current` | an error naming the file; nothing is guessed |
//!
//! # A key's life
//!
//! [`FsKeys::rotate`] makes a new key and makes it current. **The old keys stay**,
//! so that a pin made under one still verifies; nothing here deletes a key. The
//! order of the writes is the key file, then `current`: a rotation cut short leaves
//! a key nobody uses yet, never a `current` that names nothing. A key file is
//! created and never rewritten.
//!
//! Losing a key (a machine lost without a copy) means that the keyed pins made under
//! it can no longer be checked, for their owner too; approving again is the way
//! back. Copying the `keys` directory to a second machine is how a second machine
//! gets the same keys.
//!
//! # Randomness
//!
//! A key is 32 bytes drawn from the operating system's random source (through the
//! UUID library's version 4, twice: 244 bits of it). The kid is drawn apart from the
//! key, so that the public name says nothing of the secret.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use whyshelf_core::app::ports::KeySource;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::hash::PinKey;
use whyshelf_core::domain::keyring::{Kid, PinKeyring};
use whyshelf_core::domain::knowable::Knowable;

use crate::config::{ConfigError, UserDirs};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const KEY_BYTES: usize = 32;
const CURRENT_FILE: &str = "current";
const KEY_SUFFIX: &str = ".key";

/// Why the keys could not be read or made.
#[derive(Debug, thiserror::Error)]
pub enum KeysError {
    /// A file or directory could not be used.
    #[error("cannot use {path} for the keys")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// What is on disk contradicts itself or the rules of a key.
    #[error("the keys are corrupt at {path}: {detail}")]
    Corrupt {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        detail: &'static str,
    },
    /// A key was asked for where there is no place to keep one.
    #[error(
        "no data dir to keep a key in: none of WHYSHELF_DATA_DIR, XDG_DATA_HOME and HOME is an absolute path"
    )]
    NoPlace,
    /// A value read or made is refused by its rule.
    #[error("a key or its name is refused")]
    Value(#[source] ValueError),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> KeysError + '_ {
    move |source| KeysError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The person's keys, in files.
#[derive(Debug, Clone)]
pub struct FsKeys {
    dir: Option<PathBuf>,
}

impl FsKeys {
    /// The keys of the person this environment describes. A relative
    /// `WHYSHELF_DATA_DIR` is an error; an environment with no data dir at all is
    /// not (it has no place to look, which [`KeySource::keys`] answers as unknown).
    pub fn from_dirs(dirs: &UserDirs) -> Result<Self, ConfigError> {
        match dirs.data_dir() {
            Ok(dir) => Ok(Self::at(dir.join("keys"))),
            Err(ConfigError::NoDataDir) => Ok(Self { dir: None }),
            Err(other) => Err(other),
        }
    }

    /// The keys in this directory (the `keys` directory itself).
    pub fn at(dir: PathBuf) -> Self {
        Self { dir: Some(dir) }
    }

    /// The directory the keys are in, if the environment has one.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The keyring if it has a current key; otherwise make the first key.
    ///
    /// Returns the kid of the current key. Safe to call again: it changes nothing
    /// once there is a key.
    pub async fn ensure_key(&self) -> Result<Kid, KeysError> {
        let dir = self.dir.as_deref().ok_or(KeysError::NoPlace)?;
        match read_current(dir).await? {
            Some(kid) => Ok(kid),
            None => self.make_key(dir).await,
        }
    }

    /// Make a new key and make it current, keeping the old ones. Returns its kid.
    pub async fn rotate(&self) -> Result<Kid, KeysError> {
        let dir = self.dir.as_deref().ok_or(KeysError::NoPlace)?;
        self.make_key(dir).await
    }

    async fn make_key(&self, dir: &Path) -> Result<Kid, KeysError> {
        make_private_dir(dir).await?;
        let bytes = random_key();
        PinKey::new(&bytes).map_err(KeysError::Value)?;
        let kid = fresh_kid()?;
        let path = dir.join(format!("{}{KEY_SUFFIX}", kid.as_str()));
        let mut text = hex(&bytes);
        text.push('\n');
        create_private_file(&path, text.as_bytes()).await?;
        write_current(dir, &kid).await?;
        Ok(kid)
    }
}

impl KeySource for FsKeys {
    type Error = KeysError;

    async fn keys(&self) -> Result<Knowable<PinKeyring>, KeysError> {
        let Some(dir) = self.dir.as_deref() else {
            return Ok(Knowable::Unknown);
        };
        read_keyring(dir).await.map(Knowable::Known)
    }
}

async fn read_current(dir: &Path) -> Result<Option<Kid>, KeysError> {
    let path = dir.join(CURRENT_FILE);
    match tokio::fs::read_to_string(&path).await {
        Ok(text) => text
            .trim()
            .parse::<Kid>()
            .map(Some)
            .map_err(|_| KeysError::Corrupt {
                path,
                detail: "it does not hold a key name",
            }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(&path)(error)),
    }
}

async fn read_key(path: &Path) -> Result<PinKey, KeysError> {
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(io_error(path))?;
    let corrupt = |detail| KeysError::Corrupt {
        path: path.to_path_buf(),
        detail,
    };
    let bytes = unhex(text.trim()).ok_or_else(|| corrupt("it is not hexadecimal digits"))?;
    if bytes.len() != KEY_BYTES {
        return Err(corrupt("it is not 32 bytes"));
    }
    PinKey::new(&bytes).map_err(|_| corrupt("it is too short for a key"))
}

async fn read_keyring(dir: &Path) -> Result<PinKeyring, KeysError> {
    let mut listing = match tokio::fs::read_dir(dir).await {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(PinKeyring::empty()),
        Err(error) => return Err(io_error(dir)(error)),
    };
    let mut files: Vec<(Kid, PathBuf)> = Vec::new();
    while let Some(entry) = listing.next_entry().await.map_err(io_error(dir))? {
        let path = entry.path();
        let name = entry.file_name();
        let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(KEY_SUFFIX)) else {
            continue;
        };
        let kid: Kid = stem.parse().map_err(|_| KeysError::Corrupt {
            path: path.clone(),
            detail: "the file name is not a key name",
        })?;
        files.push((kid, path));
    }
    files.sort();
    let current = read_current(dir).await?;
    let Some(current) = current else {
        return match files.first() {
            None => Ok(PinKeyring::empty()),
            Some((_, path)) => Err(KeysError::Corrupt {
                path: path.clone(),
                detail: "there are keys and no `current` to say which one is in use",
            }),
        };
    };
    let mut keyring: Option<PinKeyring> = None;
    let mut retired: Vec<(Kid, PinKey)> = Vec::new();
    for (kid, path) in &files {
        let key = read_key(path).await?;
        if *kid == current {
            keyring = Some(PinKeyring::new(kid.clone(), key));
        } else {
            retired.push((kid.clone(), key));
        }
    }
    let mut keyring = keyring.ok_or_else(|| KeysError::Corrupt {
        path: dir.join(CURRENT_FILE),
        detail: "it names a key that has no file",
    })?;
    for (kid, key) in retired {
        keyring = keyring.with_retired(kid, key).map_err(KeysError::Value)?;
    }
    Ok(keyring)
}

#[cfg(unix)]
async fn make_private_dir(dir: &Path) -> Result<(), KeysError> {
    tokio::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .await
        .map_err(io_error(dir))
}

#[cfg(not(unix))]
async fn make_private_dir(dir: &Path) -> Result<(), KeysError> {
    tokio::fs::create_dir_all(dir).await.map_err(io_error(dir))
}

/// Create a file that must not exist, readable by its owner only, and write `bytes`.
async fn create_private_file(path: &Path, bytes: &[u8]) -> Result<(), KeysError> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).await.map_err(io_error(path))?;
    let written = file.write_all(bytes).await;
    let flushed = match written {
        Ok(()) => file.flush().await,
        Err(error) => Err(error),
    };
    if let Err(error) = flushed {
        drop(file);
        let _ = tokio::fs::remove_file(path).await;
        return Err(io_error(path)(error));
    }
    Ok(())
}

/// Replace `current` with the kid, through a file of its own and a rename.
async fn write_current(dir: &Path, kid: &Kid) -> Result<(), KeysError> {
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(
        "{CURRENT_FILE}.tmp-{}-{counter}",
        std::process::id()
    ));
    create_private_file(&tmp, format!("{kid}\n").as_bytes()).await?;
    let target = dir.join(CURRENT_FILE);
    if let Err(error) = tokio::fs::rename(&tmp, &target).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(io_error(&target)(error));
    }
    Ok(())
}

fn random_key() -> [u8; KEY_BYTES] {
    let mut bytes = [0u8; KEY_BYTES];
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes
}

fn fresh_kid() -> Result<Kid, KeysError> {
    let draw = Uuid::new_v4().simple().to_string();
    format!("k-{}", &draw[..12])
        .parse()
        .map_err(KeysError::Value)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            let pair = &text[i..i + 2];
            if pair.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                u8::from_str_radix(pair, 16).ok()
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use whyshelf_core::domain::hash::ContentHash;
    use whyshelf_core::domain::id::EntityId;

    fn upstream() -> EntityId {
        "0192f3a4-5b6c-7d8e-8f90-123456789abc".parse().unwrap()
    }

    #[tokio::test]
    async fn reading_never_makes_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let keys = FsKeys::at(dir.path().join("keys"));
        let ring = keys.keys().await.unwrap();
        let ring = ring.known().expect("a known keyring");
        assert!(ring.current().is_none());
        assert!(
            !dir.path().join("keys").exists(),
            "a read left nothing behind"
        );
    }

    #[tokio::test]
    async fn an_environment_without_a_data_dir_is_unknown() {
        let keys = FsKeys::from_dirs(&UserDirs::default()).unwrap();
        assert!(keys.dir().is_none());
        assert!(matches!(keys.keys().await.unwrap(), Knowable::Unknown));
        assert!(matches!(keys.ensure_key().await, Err(KeysError::NoPlace)));
        // The data dir gives the place, under `keys`.
        let keys = FsKeys::from_dirs(&UserDirs::default().with_data_dir("/data")).unwrap();
        assert_eq!(keys.dir(), Some(Path::new("/data/keys")));
        assert!(
            FsKeys::from_dirs(&UserDirs::default().with_data_dir("relative")).is_err(),
            "a relative data dir is the person's mistake and is said"
        );
    }

    #[tokio::test]
    async fn the_first_key_is_made_on_purpose_and_once() {
        let dir = tempfile::tempdir().unwrap();
        let keys = FsKeys::at(dir.path().join("keys"));
        let first = keys.ensure_key().await.unwrap();
        assert_eq!(keys.ensure_key().await.unwrap(), first, "no second key");

        let ring = keys.keys().await.unwrap();
        let ring = ring.known().unwrap();
        assert_eq!(ring.current(), Some(&first));
        let hash = ContentHash::raw(b"upstream text");
        let pin = ring.pin(upstream(), &hash).expect("a pin under the key");
        assert_eq!(pin.kid(), &first);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_directory_and_the_key_are_for_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let keys = FsKeys::at(dir.path().join("keys"));
        let kid = keys.ensure_key().await.unwrap();
        let mode =
            async |p: PathBuf| tokio::fs::metadata(p).await.unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(dir.path().join("keys")).await, 0o700);
        assert_eq!(
            mode(dir.path().join(format!("keys/{kid}.key"))).await,
            0o600
        );
        assert_eq!(mode(dir.path().join("keys/current")).await, 0o600);
    }

    #[tokio::test]
    async fn a_rotation_keeps_the_old_key_so_an_old_pin_still_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let keys = FsKeys::at(dir.path().join("keys"));
        let old = keys.ensure_key().await.unwrap();
        let hash = ContentHash::raw(b"upstream text");
        let old_pin = keys
            .keys()
            .await
            .unwrap()
            .known()
            .unwrap()
            .pin(upstream(), &hash)
            .unwrap();

        let new = keys.rotate().await.unwrap();
        assert_ne!(new, old);
        let ring = keys.keys().await.unwrap();
        let ring = ring.known().unwrap();
        assert_eq!(ring.current(), Some(&new));
        assert!(ring.knows(&old), "the retired key is kept");
        assert_eq!(
            ring.check(&old_pin, upstream(), &hash),
            whyshelf_core::domain::keyring::PinMatch::Same
        );
        assert!(
            dir.path().join(format!("keys/{old}.key")).exists(),
            "nothing deleted the old key file"
        );
    }

    #[tokio::test]
    async fn what_is_on_disk_and_makes_no_sense_is_an_error_not_a_guess() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keys");
        let keys = FsKeys::at(root.clone());
        let kid = keys.ensure_key().await.unwrap();

        // Keys and no `current`.
        tokio::fs::remove_file(root.join("current")).await.unwrap();
        assert!(matches!(keys.keys().await, Err(KeysError::Corrupt { .. })));
        // A `current` that names no key.
        tokio::fs::write(root.join("current"), "k-000000000000\n")
            .await
            .unwrap();
        assert!(matches!(keys.keys().await, Err(KeysError::Corrupt { .. })));
        // A key that is not 32 bytes of hex.
        tokio::fs::write(root.join("current"), format!("{kid}\n"))
            .await
            .unwrap();
        tokio::fs::write(root.join(format!("{kid}.key")), "abcd\n")
            .await
            .unwrap();
        assert!(matches!(keys.keys().await, Err(KeysError::Corrupt { .. })));
    }

    #[test]
    fn hex_round_trips_and_refuses_what_is_not_lowercase_hex() {
        let bytes = [0u8, 1, 0xab, 0xff];
        assert_eq!(unhex(&hex(&bytes)).unwrap(), bytes);
        assert!(unhex("AB").is_none());
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
    }
}
