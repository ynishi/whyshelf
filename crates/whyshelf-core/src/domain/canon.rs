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
//! # Text rule
//!
//! Text is normalized once, by the adapter that reads a file, before it is
//! parsed: line breaks become LF, the trailing spaces and tabs of each line are
//! removed, and the end of the file becomes exactly one LF. Nothing else is
//! changed (no Unicode normalization). The domain receives text already in this
//! form and does not repeat the step.

use std::collections::BTreeMap;

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

impl Canon {
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

/// A value that has one canonical form.
///
/// Crate-private: the canonical forms are an implementation of the hash scope
/// documented on each entity, not an extension point.
pub(crate) trait ToCanon {
    /// The canonical form of this value.
    fn to_canon(&self) -> Canon;
}

impl<T: ToCanon> ToCanon for Option<T> {
    fn to_canon(&self) -> Canon {
        self.as_ref().map_or(Canon::Null, ToCanon::to_canon)
    }
}

impl<T: ToCanon> ToCanon for [T] {
    fn to_canon(&self) -> Canon {
        Canon::list(self.iter().map(ToCanon::to_canon))
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

    #[test]
    fn option_none_is_null() {
        struct One;
        impl ToCanon for One {
            fn to_canon(&self) -> Canon {
                Canon::Int(1)
            }
        }
        assert_eq!(None::<One>.to_canon(), Canon::Null);
        assert_eq!(Some(One).to_canon(), Canon::Int(1));
    }
}
