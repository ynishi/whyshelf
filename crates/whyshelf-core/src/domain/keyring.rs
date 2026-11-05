//! Keys for the pins of private upstreams, and the pins they make.
//!
//! An approval pins what it stands on: the hash of each upstream as it was. When the
//! upstream is a private document and the approval is written where others read
//! (the public ledger), a plain hash would give away a fingerprint of the private
//! text. The pin is then a [`KeyedPin`]: the id of a key ([`Kid`]) and the HMAC of
//! the upstream's hash under that key ([`PinMac`], computed in
//! [`hash`](super::hash)). It is made so from the start, whichever ledger the record
//! goes to, so publishing a document never rewrites a record.
//!
//! ```text
//!   upstream private?  --yes--> keyring has a current key? --yes--> KeyedPin { kid, mac }
//!        |                                |
//!        no                               no --> cannot be pinned (the gate says so)
//!        v
//!   plain ContentHash
//! ```
//!
//! # Comparing a pin with the upstream today
//!
//! [`UpstreamHash::check`] says one of three things ([`PinMatch`]):
//!
//! | recorded pin | the keyring | answer |
//! |---|---|---|
//! | plain | (not asked) | `Same` or `Differs` by the hashes |
//! | keyed, `kid` in the keyring | recompute the MAC of today's hash | `Same` or `Differs` |
//! | keyed, `kid` not in the keyring | cannot recompute | `Unknown`, naming the `kid` |
//!
//! `Unknown` is not `Differs`. A person who lost the key, or a colleague who never
//! had it, cannot tell whether the upstream changed, and saying "stale" would send
//! them to approve again something that may be fine. The evaluation shows such an
//! entity as Partial.
//!
//! # The keyring
//!
//! A [`PinKeyring`] holds the keys of one person, one of them current. New pins are
//! made under the current key. Rotating ([`PinKeyring::rotated`]) makes a new key
//! current and keeps the old ones, so a pin made earlier still verifies.
//!
//! The keys themselves are values passed in. Where they live (a file in the
//! person's own data, outside the project) and what losing one costs (every pin
//! made under it can no longer be checked; approving again is the way back) are
//! for the application and the documentation of the command line.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use super::canon::{Canon, ToCanon};
use super::error::ValueError;
use super::hash::{ContentHash, PinKey, PinMac};
use super::id::EntityId;

const KID_MAX: usize = 64;

/// The name of a key: what a keyed pin says it was made under.
///
/// 1 to 64 characters of lowercase letters, digits and `. _ -`. It is public (it is
/// written in the ledger beside the pin) and names a key without revealing it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Kid(String);

impl Kid {
    /// The text of the name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Kid {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let shaped = !s.is_empty()
            && s.len() <= KID_MAX
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_".contains(c));
        if shaped {
            Ok(Self(s.to_owned()))
        } else {
            Err(ValueError::KidBadShape)
        }
    }
}

impl fmt::Display for Kid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ToCanon for Kid {
    fn to_canon(&self) -> Canon {
        Canon::str(&self.0)
    }
}

/// A pin of a private upstream: the key it was made under and the MAC of the
/// upstream's hash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyedPin {
    kid: Kid,
    mac: PinMac,
}

impl KeyedPin {
    /// A pin from its two parts, as a reader of a record reconstructs it.
    pub fn new(kid: Kid, mac: PinMac) -> Self {
        Self { kid, mac }
    }

    /// The key it was made under.
    pub fn kid(&self) -> &Kid {
        &self.kid
    }

    /// The MAC.
    pub fn mac(&self) -> &PinMac {
        &self.mac
    }
}

impl ToCanon for KeyedPin {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("kid", self.kid.to_canon()),
            ("mac", Canon::str(self.mac.hex())),
        ])
    }
}

/// What a recorded pin comes to against the upstream as it is today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinMatch {
    /// The upstream is as it was.
    Same,
    /// The upstream is not as it was.
    Differs,
    /// It cannot be told here: the pin is keyed and the keyring has no such key.
    Unknown {
        /// The key the pin was made under.
        kid: Kid,
    },
}

/// The hash an approval recorded for an upstream: the hash itself, or, for a private
/// upstream, a keyed pin in its place.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UpstreamHash {
    /// The hash as it is.
    Plain(ContentHash),
    /// A MAC of it under a key.
    Keyed(KeyedPin),
}

impl UpstreamHash {
    /// The plain hash, if the pin is plain.
    pub fn plain(&self) -> Option<&ContentHash> {
        match self {
            Self::Plain(hash) => Some(hash),
            Self::Keyed(_) => None,
        }
    }

    /// The keyed pin, if the pin is keyed.
    pub fn keyed(&self) -> Option<&KeyedPin> {
        match self {
            Self::Plain(_) => None,
            Self::Keyed(pin) => Some(pin),
        }
    }

    /// What this pin of the upstream `upstream` comes to against its `current` hash.
    pub fn check(
        &self,
        upstream: EntityId,
        current: &ContentHash,
        keyring: &PinKeyring,
    ) -> PinMatch {
        match self {
            Self::Plain(recorded) if recorded == current => PinMatch::Same,
            Self::Plain(_) => PinMatch::Differs,
            Self::Keyed(pin) => keyring.check(pin, upstream, current),
        }
    }
}

impl From<ContentHash> for UpstreamHash {
    fn from(hash: ContentHash) -> Self {
        Self::Plain(hash)
    }
}

impl From<KeyedPin> for UpstreamHash {
    fn from(pin: KeyedPin) -> Self {
        Self::Keyed(pin)
    }
}

impl ToCanon for UpstreamHash {
    /// A plain pin has the form a hash always had, so a record that pins nothing
    /// private keeps the canonical bytes it had before keyed pins existed.
    fn to_canon(&self) -> Canon {
        match self {
            Self::Plain(hash) => hash.to_canon(),
            Self::Keyed(pin) => pin.to_canon(),
        }
    }
}

/// A private upstream cannot be pinned: the keyring has no key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoKey;

/// The keys of one person, one of them current.
///
/// Built from values: the domain reads no file. See the module documentation for
/// what each answer means.
#[derive(Debug, Clone, Default)]
pub struct PinKeyring {
    keys: BTreeMap<Kid, PinKey>,
    current: Option<Kid>,
}

impl PinKeyring {
    /// A keyring with no key: it can verify no keyed pin and make none.
    ///
    /// This is the keyring of a person (or a machine, a CI run) who has no key.
    pub const fn empty() -> Self {
        Self {
            keys: BTreeMap::new(),
            current: None,
        }
    }

    /// A keyring with one key, which is current.
    pub fn new(kid: Kid, key: PinKey) -> Self {
        Self {
            current: Some(kid.clone()),
            keys: BTreeMap::from([(kid, key)]),
        }
    }

    /// The same keyring with a new key that is current. The keys it had stay, so
    /// the pins made under them still verify.
    pub fn rotated(mut self, kid: Kid, key: PinKey) -> Result<Self, ValueError> {
        self.insert(kid.clone(), key)?;
        self.current = Some(kid);
        Ok(self)
    }

    /// The same keyring with an earlier key added, not current: for a keyring read
    /// back from where the person keeps their keys.
    pub fn with_retired(mut self, kid: Kid, key: PinKey) -> Result<Self, ValueError> {
        self.insert(kid, key)?;
        Ok(self)
    }

    fn insert(&mut self, kid: Kid, key: PinKey) -> Result<(), ValueError> {
        if self.keys.contains_key(&kid) {
            return Err(ValueError::KidDuplicate {
                kid: kid.to_string(),
            });
        }
        self.keys.insert(kid, key);
        Ok(())
    }

    /// The key new pins are made under; none for an empty keyring.
    pub fn current(&self) -> Option<&Kid> {
        self.current.as_ref()
    }

    /// Whether the keyring has the key.
    pub fn knows(&self, kid: &Kid) -> bool {
        self.keys.contains_key(kid)
    }

    /// The pin of the upstream `upstream` at `hash` under the current key; `None`
    /// when there is no key, which is why a private upstream then cannot be pinned.
    pub fn pin(&self, upstream: EntityId, hash: &ContentHash) -> Option<KeyedPin> {
        let kid = self.current.as_ref()?;
        let key = self.keys.get(kid)?;
        Some(KeyedPin::new(kid.clone(), PinMac::of(key, upstream, hash)))
    }

    /// The pin of an upstream at `hash`, as the one rule for it says: a keyed pin
    /// under the current key when the upstream is `private`, the plain hash when it
    /// is not. The one place that decides keyed or plain.
    ///
    /// # Errors
    ///
    /// [`NoKey`] when the upstream is private and the keyring has no key.
    pub fn pin_upstream(
        &self,
        private: bool,
        upstream: EntityId,
        hash: &ContentHash,
    ) -> Result<UpstreamHash, NoKey> {
        if private {
            self.pin(upstream, hash)
                .map(UpstreamHash::Keyed)
                .ok_or(NoKey)
        } else {
            Ok(UpstreamHash::Plain(*hash))
        }
    }

    /// What a keyed pin comes to against the upstream's `current` hash.
    pub fn check(&self, pin: &KeyedPin, upstream: EntityId, current: &ContentHash) -> PinMatch {
        match self.keys.get(&pin.kid) {
            None => PinMatch::Unknown {
                kid: pin.kid.clone(),
            },
            Some(key) if PinMac::of(key, upstream, current) == pin.mac => PinMatch::Same,
            Some(_) => PinMatch::Differs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::id::test_support::eid;

    fn kid(text: &str) -> Kid {
        text.parse().unwrap()
    }

    fn key(byte: u8) -> PinKey {
        PinKey::new(&[byte; 32]).unwrap()
    }

    fn hash(text: &str) -> ContentHash {
        ContentHash::raw(text.as_bytes())
    }

    #[test]
    fn a_private_upstream_is_pinned_keyed_and_a_public_one_plain() {
        let ring = PinKeyring::new(kid("k1"), key(1));
        let h = hash("v1");
        assert_eq!(
            ring.pin_upstream(false, eid(1), &h),
            Ok(UpstreamHash::Plain(h))
        );
        assert_eq!(
            ring.pin_upstream(true, eid(1), &h),
            Ok(UpstreamHash::Keyed(ring.pin(eid(1), &h).unwrap()))
        );
        // No key: a public upstream still pins, a private one cannot.
        let none = PinKeyring::empty();
        assert_eq!(
            none.pin_upstream(false, eid(1), &h),
            Ok(UpstreamHash::Plain(h))
        );
        assert_eq!(none.pin_upstream(true, eid(1), &h), Err(NoKey));
    }

    #[test]
    fn a_kid_has_a_shape() {
        assert!("k1".parse::<Kid>().is_ok());
        assert!("2026.key-a_b".parse::<Kid>().is_ok());
        for bad in ["", "K1", "a b", "é", &"a".repeat(65)] {
            assert_eq!(bad.parse::<Kid>(), Err(ValueError::KidBadShape), "{bad:?}");
        }
    }

    #[test]
    fn a_pin_under_a_key_is_the_same_while_the_upstream_is_and_differs_after() {
        let ring = PinKeyring::new(kid("k1"), key(1));
        let pin = UpstreamHash::from(ring.pin(eid(1), &hash("v1")).unwrap());
        assert_eq!(pin.check(eid(1), &hash("v1"), &ring), PinMatch::Same);
        assert_eq!(pin.check(eid(1), &hash("v2"), &ring), PinMatch::Differs);
    }

    #[test]
    fn a_pin_made_under_a_key_the_keyring_lacks_is_unknown_not_differs() {
        let theirs = PinKeyring::new(kid("k1"), key(1));
        let pin = UpstreamHash::from(theirs.pin(eid(1), &hash("v1")).unwrap());
        let ours = PinKeyring::new(kid("k2"), key(2));
        // The answer names the key that is lacking.
        let unknown = PinMatch::Unknown { kid: kid("k1") };
        assert_eq!(pin.check(eid(1), &hash("v1"), &ours), unknown);
        assert_eq!(
            pin.check(eid(1), &hash("v1"), &PinKeyring::empty()),
            unknown
        );
    }

    #[test]
    fn the_same_kid_with_another_key_differs() {
        let theirs = PinKeyring::new(kid("k1"), key(1));
        let pin = UpstreamHash::from(theirs.pin(eid(1), &hash("v1")).unwrap());
        let ours = PinKeyring::new(kid("k1"), key(9));
        assert_eq!(pin.check(eid(1), &hash("v1"), &ours), PinMatch::Differs);
    }

    #[test]
    fn rotating_keeps_the_old_pins_verifying_and_makes_new_ones_under_the_new_key() {
        let old = PinKeyring::new(kid("k1"), key(1));
        let before = UpstreamHash::from(old.pin(eid(1), &hash("v1")).unwrap());
        let rotated = old.rotated(kid("k2"), key(2)).unwrap();
        assert_eq!(rotated.current(), Some(&kid("k2")));
        assert_eq!(before.check(eid(1), &hash("v1"), &rotated), PinMatch::Same);
        assert_eq!(
            before.check(eid(1), &hash("v2"), &rotated),
            PinMatch::Differs
        );
        let after = rotated.pin(eid(1), &hash("v1")).unwrap();
        assert_eq!(after.kid(), &kid("k2"));
        assert_ne!(after.mac(), before.keyed().unwrap().mac());
    }

    #[test]
    fn a_key_id_cannot_be_listed_twice() {
        let ring = PinKeyring::new(kid("k1"), key(1));
        assert_eq!(
            ring.clone().rotated(kid("k1"), key(2)).unwrap_err(),
            ValueError::KidDuplicate { kid: "k1".into() }
        );
        assert!(ring.with_retired(kid("k0"), key(0)).is_ok());
    }

    #[test]
    fn an_empty_keyring_makes_no_pin() {
        assert!(PinKeyring::empty().pin(eid(1), &hash("v1")).is_none());
        assert!(PinKeyring::empty().current().is_none());
    }

    #[test]
    fn a_plain_pin_compares_the_hashes_and_ignores_the_keyring() {
        let pin = UpstreamHash::from(hash("v1"));
        let ring = PinKeyring::empty();
        assert_eq!(pin.check(eid(1), &hash("v1"), &ring), PinMatch::Same);
        assert_eq!(pin.check(eid(1), &hash("v2"), &ring), PinMatch::Differs);
    }

    #[test]
    fn a_plain_pin_keeps_the_canonical_form_of_a_hash() {
        let h = hash("v1");
        assert_eq!(UpstreamHash::from(h).to_canon(), h.to_canon());
    }

    #[test]
    fn a_keyed_pin_shows_no_hash_of_the_upstream() {
        let ring = PinKeyring::new(kid("k1"), key(1));
        let h = hash("secret text");
        let pin = UpstreamHash::from(ring.pin(eid(1), &h).unwrap());
        let written = String::from_utf8(pin.to_canon().to_bytes()).unwrap();
        assert!(!written.contains(&h.hex()));
        assert!(written.contains("k1"));
    }
}
