//! The canonical form of a value and its deterministic byte encoding.
//!
//! Every hash in whyshelf is computed from a [`Canon`], never from a
//! serialization library's output. A serializer's field order, escaping and
//! number formatting can change between versions; a hash that must stay equal
//! for years cannot depend on them. The encoding here is fixed by this file
//! alone.
//!
//! # Shape of a value
//!
//! [`Canon`] has six cases: null, boolean, 64-bit integer, string, list and map.
//! There is no floating point number: a measurement is an integer or a string.
//! A map is ordered by key (it is a `BTreeMap`), so the order a caller inserts
//! keys in cannot change the bytes. A string is UTF-8 as given; no Unicode
//! normalization is applied, because the rule for text is only about white space
//! and line breaks and is applied before parsing (see the text rule below).
//!
//! # Byte encoding
//!
//! Each value is `tag (1 byte) + length (u64, big endian) + body`:
//!
//! | case | tag | length | body |
//! |---|---|---|---|
//! | null | `0x00` | 0 | empty |
//! | boolean | `0x01` | 1 | `0x00` or `0x01` |
//! | integer | `0x02` | 8 | the `i64` as 8 big-endian bytes |
//! | string | `0x03` | number of bytes | the UTF-8 bytes |
//! | list | `0x04` | number of elements | the encoded elements, in order |
//! | map | `0x05` | number of entries | per entry, the encoded key (a string) then the encoded value, in key order |
//!
//! The encoding is self-delimiting, so two different values never share bytes,
//! and it is not JSON, so there is no escaping to disagree about.
//!
//! # Reading the bytes back
//!
//! [`Canon::from_bytes`] is the one decoder. It is strict: the value it reads is
//! encoded again, and the bytes must be the ones it came from. That refuses, with no
//! rule of its own for each, trailing bytes, map keys out of order or repeated, and
//! every other spelling of a value that [`Canon::to_bytes`] would not write. A list or
//! map whose length claims more entries than the bytes left could hold is refused
//! before anything is allocated, and values nested deeper than [`Canon::MAX_DEPTH`]
//! are refused, so bytes from a file cannot make the reader run out of memory or
//! stack.
//!
//! # Text rule
//!
//! Text is normalized once, by the adapter that reads a file, before it is
//! parsed: line breaks become LF, the trailing spaces and tabs of each line are
//! removed, and the end of the file becomes exactly one LF. Nothing else is
//! changed (no Unicode normalization). The domain receives text already in this
//! form and does not repeat the step.

use std::collections::BTreeMap;

use thiserror::Error;

const TAG_NULL: u8 = 0x00;
const TAG_BOOL: u8 = 0x01;
const TAG_INT: u8 = 0x02;
const TAG_STR: u8 = 0x03;
const TAG_LIST: u8 = 0x04;
const TAG_MAP: u8 = 0x05;

/// A value in canonical form, ready to be hashed.
///
/// Why an enum of our own: the hash of an approved entity must not move when a
/// dependency is upgraded, so the form being hashed is defined here and not by
/// a serialization framework.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Canon {
    /// The absence of a value.
    Null,
    /// A truth value.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// A text, UTF-8, kept as given.
    Str(String),
    /// A sequence whose order matters.
    List(Vec<Canon>),
    /// A mapping ordered by key.
    Map(BTreeMap<String, Canon>),
}

/// Why bytes are not the canonical encoding of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CanonError {
    /// The bytes end before the value does.
    #[error("the bytes end before the value does")]
    Truncated,
    /// A value starts with a tag no case has.
    #[error("{tag:#04x} is not the tag of a value")]
    UnknownTag {
        /// The byte found.
        tag: u8,
    },
    /// The length does not go with the tag (a boolean of two bytes, a list longer
    /// than the bytes left).
    #[error("the length of a value does not go with its tag")]
    BadLength,
    /// A boolean that is neither `0x00` nor `0x01`.
    #[error("a boolean is 0 or 1")]
    BadBool,
    /// A string that is not UTF-8.
    #[error("a string is not UTF-8")]
    NotUtf8,
    /// A map key that is not a string.
    #[error("a map key is not a string")]
    KeyNotString,
    /// Values nested deeper than [`Canon::MAX_DEPTH`].
    #[error("values are nested deeper than {}", Canon::MAX_DEPTH)]
    TooDeep,
    /// Bytes after the value.
    #[error("there are bytes after the value")]
    Trailing,
    /// A value that encodes to other bytes than it was read from (keys out of order
    /// or repeated).
    #[error("the bytes are not the form the encoder writes")]
    NotCanonical,
}

impl Canon {
    /// The deepest nesting of lists and maps [`from_bytes`](Self::from_bytes) reads.
    /// A record is a few levels deep.
    pub const MAX_DEPTH: usize = 32;

    /// A string value.
    pub fn str(text: impl Into<String>) -> Self {
        Self::Str(text.into())
    }

    /// A list value from any iterator of values.
    pub fn list(items: impl IntoIterator<Item = Canon>) -> Self {
        Self::List(items.into_iter().collect())
    }

    /// A map value from pairs. A repeated key keeps the last value.
    pub fn map<K: Into<String>>(entries: impl IntoIterator<Item = (K, Canon)>) -> Self {
        Self::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// The deterministic bytes of this value.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }

    /// The value whose canonical encoding these bytes are.
    ///
    /// Strict, as the module documentation says: `Canon::from_bytes(b)` is `Ok`
    /// exactly when `b` is what [`to_bytes`](Self::to_bytes) writes for some value.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CanonError> {
        let mut rest = bytes;
        let value = take_value(&mut rest, 0)?;
        if !rest.is_empty() {
            return Err(CanonError::Trailing);
        }
        if value.to_bytes() != bytes {
            return Err(CanonError::NotCanonical);
        }
        Ok(value)
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Null => header(out, TAG_NULL, 0),
            Self::Bool(b) => {
                header(out, TAG_BOOL, 1);
                out.push(u8::from(*b));
            }
            Self::Int(n) => {
                header(out, TAG_INT, 8);
                out.extend_from_slice(&n.to_be_bytes());
            }
            Self::Str(s) => write_str(out, s),
            Self::List(items) => {
                header(out, TAG_LIST, items.len() as u64);
                for item in items {
                    item.write(out);
                }
            }
            Self::Map(entries) => {
                header(out, TAG_MAP, entries.len() as u64);
                for (key, value) in entries {
                    write_str(out, key);
                    value.write(out);
                }
            }
        }
    }
}

fn header(out: &mut Vec<u8>, tag: u8, len: u64) {
    out.push(tag);
    out.extend_from_slice(&len.to_be_bytes());
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    header(out, TAG_STR, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// The fewest bytes a value takes: its tag and its length.
const MIN_VALUE: usize = 9;

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], CanonError> {
    if rest.len() < n {
        return Err(CanonError::Truncated);
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn take_array<const N: usize>(rest: &mut &[u8]) -> Result<[u8; N], CanonError> {
    take(rest, N)?.try_into().map_err(|_| CanonError::Truncated)
}

fn take_value(rest: &mut &[u8], depth: usize) -> Result<Canon, CanonError> {
    if depth > Canon::MAX_DEPTH {
        return Err(CanonError::TooDeep);
    }
    let [tag] = take_array::<1>(rest)?;
    let len = u64::from_be_bytes(take_array::<8>(rest)?);
    match tag {
        TAG_NULL if len == 0 => Ok(Canon::Null),
        TAG_BOOL if len == 1 => match take_array::<1>(rest)? {
            [0] => Ok(Canon::Bool(false)),
            [1] => Ok(Canon::Bool(true)),
            _ => Err(CanonError::BadBool),
        },
        TAG_INT if len == 8 => Ok(Canon::Int(i64::from_be_bytes(take_array::<8>(rest)?))),
        TAG_NULL | TAG_BOOL | TAG_INT => Err(CanonError::BadLength),
        TAG_STR => {
            let len = usize::try_from(len).map_err(|_| CanonError::BadLength)?;
            let body = take(rest, len)?;
            std::str::from_utf8(body)
                .map(Canon::str)
                .map_err(|_| CanonError::NotUtf8)
        }
        TAG_LIST => {
            // Every element is at least a tag and a length: more elements than the
            // bytes left could hold is a length that lies, refused before anything
            // is allocated.
            let count = usize::try_from(len)
                .ok()
                .filter(|n| *n <= rest.len() / MIN_VALUE)
                .ok_or(CanonError::BadLength)?;
            (0..count)
                .map(|_| take_value(rest, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Canon::List)
        }
        TAG_MAP => {
            let count = usize::try_from(len)
                .ok()
                .filter(|n| *n <= rest.len() / (2 * MIN_VALUE))
                .ok_or(CanonError::BadLength)?;
            let mut map = BTreeMap::new();
            for _ in 0..count {
                let Canon::Str(key) = take_value(rest, depth + 1)? else {
                    return Err(CanonError::KeyNotString);
                };
                map.insert(key, take_value(rest, depth + 1)?);
            }
            Ok(Canon::Map(map))
        }
        tag => Err(CanonError::UnknownTag { tag }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_have_tag_length_body() {
        assert_eq!(
            Canon::Null.to_bytes(),
            [0x00, 0, 0, 0, 0, 0, 0, 0, 0].to_vec()
        );
        assert_eq!(
            Canon::Bool(true).to_bytes(),
            [0x01, 0, 0, 0, 0, 0, 0, 0, 1, 1].to_vec()
        );
        assert_eq!(
            Canon::Int(-2).to_bytes(),
            [
                0x02, 0, 0, 0, 0, 0, 0, 0, 8, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe
            ]
            .to_vec()
        );
        assert_eq!(
            Canon::str("hi").to_bytes(),
            [0x03, 0, 0, 0, 0, 0, 0, 0, 2, b'h', b'i'].to_vec()
        );
    }

    #[test]
    fn map_order_is_by_key_not_by_insertion() {
        let a = Canon::map([("b", Canon::Int(1)), ("a", Canon::Int(2))]);
        let b = Canon::map([("a", Canon::Int(2)), ("b", Canon::Int(1))]);
        assert_eq!(a.to_bytes(), b.to_bytes());
    }

    #[test]
    fn encoding_is_self_delimiting() {
        // ["ab", "c"] and ["a", "bc"] must differ even though the text is the same.
        let x = Canon::list([Canon::str("ab"), Canon::str("c")]);
        let y = Canon::list([Canon::str("a"), Canon::str("bc")]);
        assert_ne!(x.to_bytes(), y.to_bytes());
        // A list of one list differs from the inner list.
        let inner = Canon::list([Canon::Int(1)]);
        assert_ne!(Canon::list([inner.clone()]).to_bytes(), inner.to_bytes());
    }

    #[test]
    fn a_string_and_a_number_never_collide() {
        assert_ne!(Canon::str("1").to_bytes(), Canon::Int(1).to_bytes());
        assert_ne!(Canon::Null.to_bytes(), Canon::Bool(false).to_bytes());
    }

    fn every_case() -> Canon {
        Canon::map([
            ("null", Canon::Null),
            ("yes", Canon::Bool(true)),
            ("no", Canon::Bool(false)),
            ("int", Canon::Int(i64::MIN)),
            ("str", Canon::str("héllo")),
            (
                "list",
                Canon::list([Canon::Int(1), Canon::list([]), Canon::map::<&str>([])]),
            ),
        ])
    }

    #[test]
    fn what_the_encoder_writes_is_read_back() {
        let value = every_case();
        assert_eq!(Canon::from_bytes(&value.to_bytes()), Ok(value));
    }

    #[test]
    fn bytes_that_are_not_an_encoding_are_each_refused_for_their_own_reason() {
        let refuse = |bytes: &[u8]| Canon::from_bytes(bytes).unwrap_err();
        assert_eq!(refuse(&[]), CanonError::Truncated);
        assert_eq!(
            refuse(&[0x03, 0, 0, 0, 0, 0, 0, 0, 5, b'a']),
            CanonError::Truncated
        );
        assert_eq!(
            refuse(&[0x09, 0, 0, 0, 0, 0, 0, 0, 0]),
            CanonError::UnknownTag { tag: 9 }
        );
        assert_eq!(
            refuse(&[0x00, 0, 0, 0, 0, 0, 0, 0, 1]),
            CanonError::BadLength
        );
        assert_eq!(
            refuse(&[0x01, 0, 0, 0, 0, 0, 0, 0, 1, 2]),
            CanonError::BadBool
        );
        assert_eq!(
            refuse(&[0x03, 0, 0, 0, 0, 0, 0, 0, 1, 0xff]),
            CanonError::NotUtf8
        );
        let mut trailing = Canon::Null.to_bytes();
        trailing.push(0);
        assert_eq!(refuse(&trailing), CanonError::Trailing);
        // A map whose key is a number.
        let mut keyed = vec![0x05, 0, 0, 0, 0, 0, 0, 0, 1];
        keyed.extend(Canon::Int(1).to_bytes());
        keyed.extend(Canon::Null.to_bytes());
        assert_eq!(refuse(&keyed), CanonError::KeyNotString);
    }

    #[test]
    fn keys_out_of_order_or_repeated_are_not_the_encoding() {
        let entry = |key: &str| {
            let mut out = Canon::str(key).to_bytes();
            out.extend(Canon::Null.to_bytes());
            out
        };
        let map = |keys: &[&str]| {
            let mut out = vec![0x05, 0, 0, 0, 0, 0, 0, 0, keys.len() as u8];
            for key in keys {
                out.extend(entry(key));
            }
            out
        };
        assert!(Canon::from_bytes(&map(&["a", "b"])).is_ok());
        assert_eq!(
            Canon::from_bytes(&map(&["b", "a"])),
            Err(CanonError::NotCanonical)
        );
        assert_eq!(
            Canon::from_bytes(&map(&["a", "a"])),
            Err(CanonError::NotCanonical)
        );
    }

    #[test]
    fn a_length_that_lies_costs_nothing() {
        // A list that says it holds 2^60 elements, in 9 bytes.
        let mut lying = vec![0x04];
        lying.extend((1u64 << 60).to_be_bytes());
        assert_eq!(Canon::from_bytes(&lying), Err(CanonError::BadLength));
        // A string that says it is 2^60 bytes long.
        let mut long = vec![0x03];
        long.extend((1u64 << 60).to_be_bytes());
        assert_eq!(Canon::from_bytes(&long), Err(CanonError::Truncated));
    }

    #[test]
    fn nesting_is_bounded() {
        let nested = |depth: usize| (0..depth).fold(Canon::Null, |inner, _| Canon::list([inner]));
        let ok = nested(Canon::MAX_DEPTH);
        assert_eq!(Canon::from_bytes(&ok.to_bytes()), Ok(ok));
        let deep = nested(Canon::MAX_DEPTH + 2);
        assert_eq!(
            Canon::from_bytes(&deep.to_bytes()),
            Err(CanonError::TooDeep)
        );
    }
}
