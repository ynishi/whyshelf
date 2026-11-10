//! The entries of a declaration in the order it writes them, found by name.

use std::borrow::Borrow;
use std::collections::BTreeMap;

/// Named entries (books, fields, links) in the order the declaration writes them,
/// each name once, with a lookup by name.
///
/// The order is kept once, in the list of entries: the lookup holds only where
/// each name is in the list, and both are written by one method, so neither can
/// drift from the other or leave an entry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared<K, V> {
    entries: Vec<(K, V)>,
    index: BTreeMap<K, usize>,
}

impl<K, V> Default for Declared<K, V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: BTreeMap::new(),
        }
    }
}

impl<K: Ord + Clone, V> Declared<K, V> {
    /// Add an entry after the others; a name already there keeps its first entry,
    /// and the answer is false.
    pub(in crate::domain::shelf) fn insert(&mut self, key: K, value: V) -> bool {
        if self.index.contains_key(&key) {
            return false;
        }
        self.index.insert(key.clone(), self.entries.len());
        self.entries.push((key, value));
        true
    }

    /// The entry of a name, with the name.
    pub fn get_key_value<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let (k, v) = &self.entries[*self.index.get(key)?];
        Some((k, v))
    }

    /// The entry of a name.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.get_key_value(key).map(|(_, v)| v)
    }

    /// Whether a name has an entry.
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.index.contains_key(key)
    }
}

impl<K, V> Declared<K, V> {
    /// The entries, owned, in the order of the declaration.
    pub(in crate::domain::shelf) fn into_entries(self) -> Vec<(K, V)> {
        self.entries
    }

    /// The entries, in the order of the declaration.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// The names, in the order of the declaration.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.iter().map(|(k, _)| k)
    }

    /// The entries without their names, in the order of the declaration.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }

    /// How many entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'a, K, V> IntoIterator for &'a Declared<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, (K, V)>, fn(&'a (K, V)) -> (&'a K, &'a V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

/// The entries of one kind among the entries of a [`Declared`], in its order: the
/// links of a book into books, or to artifacts, among all its links.
#[derive(Debug, Clone, Copy)]
pub struct Picked<'a, K, V, T> {
    all: &'a Declared<K, V>,
    pick: fn(&V) -> Option<&T>,
}

impl<'a, K: Ord + Clone, V, T: 'a> Picked<'a, K, V, T> {
    pub(super) fn new(all: &'a Declared<K, V>, pick: fn(&V) -> Option<&T>) -> Self {
        Self { all, pick }
    }

    /// The entry of a name, with the name, when it is of this kind.
    pub fn get_key_value<Q>(&self, key: &Q) -> Option<(&'a K, &'a T)>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let all: &'a Declared<K, V> = self.all;
        let (k, v) = all.get_key_value(key)?;
        Some((k, (self.pick)(v)?))
    }

    /// The entry of a name, when it is of this kind.
    pub fn get<Q>(&self, key: &Q) -> Option<&'a T>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.get_key_value(key).map(|(_, v)| v)
    }

    /// Whether a name has an entry of this kind.
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.get(key).is_some()
    }

    /// The entries of this kind, in the order of the declaration.
    pub fn iter(&self) -> impl Iterator<Item = (&'a K, &'a T)> + use<'a, K, V, T> {
        let pick = self.pick;
        self.all
            .iter()
            .filter_map(move |(k, v)| Some((k, pick(v)?)))
    }

    /// The names of the entries of this kind, in the order of the declaration.
    pub fn keys(&self) -> impl Iterator<Item = &'a K> + use<'a, K, V, T> {
        self.iter().map(|(k, _)| k)
    }

    /// The entries of this kind without their names, in the order of the
    /// declaration.
    pub fn values(&self) -> impl Iterator<Item = &'a T> + use<'a, K, V, T> {
        self.iter().map(|(_, v)| v)
    }

    /// How many entries of this kind.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Whether there is none of this kind.
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}

impl<'a, K: Ord + Clone, V, T: 'a> IntoIterator for Picked<'a, K, V, T> {
    type Item = (&'a K, &'a T);
    type IntoIter = Box<dyn Iterator<Item = (&'a K, &'a T)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        let pick = self.pick;
        Box::new(
            self.all
                .entries
                .iter()
                .filter_map(move |(k, v)| Some((k, pick(v)?))),
        )
    }
}
