//! The one macro that gives a closed set of words its names.
//!
//! A closed vocabulary (an evidence label, a maturity level, a state a query can
//! name) is an enum with a list of all its values, a name for each, and the way
//! back from a name. Writing those three by hand for every enum is how the
//! adapters came to keep tables of their own; the macro writes them once, so an
//! adapter reads `ALL`, `name()` and `FromStr` and keeps no table.

/// Give an enum `ALL`, `name()` and `FromStr`.
///
/// `what` is how the error of `FromStr` calls the word.
macro_rules! closed_vocabulary {
    ($ty:ident, $what:literal, { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl $ty {
            /// Every value, in the order of the declaration.
            pub const ALL: [$ty; [$($name),+].len()] = [$($ty::$variant),+];

            /// The name a person reads and types: the display and the input.
            ///
            /// The hash and the wire format keep literals of their own, so renaming a
            /// word here can neither change an identifier nor break a stored event.
            pub fn name(self) -> &'static str {
                match self {
                    $($ty::$variant => $name),+
                }
            }
        }

        impl ::std::str::FromStr for $ty {
            type Err = $crate::domain::error::ValueError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                Self::ALL
                    .into_iter()
                    .find(|value| value.name() == text)
                    .ok_or_else(|| $crate::domain::error::ValueError::UnknownName {
                        what: $what,
                        found: text.to_owned(),
                    })
            }
        }
    };
}

pub(crate) use closed_vocabulary;
