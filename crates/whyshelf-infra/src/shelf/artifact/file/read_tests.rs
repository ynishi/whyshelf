//! The tests of the reading of the files that ask its own parts.

use super::*;

#[test]
fn a_part_is_held_against_the_names_of_its_directory() {
    use std::ffi::OsStr;
    let names = |all: &'static [&'static str]| all.iter().map(OsStr::new);
    assert_eq!(
        part_names_entry("Justfile", names(&["a", "Justfile"])),
        PartName::Written
    );
    assert_eq!(
        part_names_entry("justfile", names(&["a", "Justfile"])),
        PartName::OtherCase
    );
    // Both spellings there: the one written is a file of its own.
    assert_eq!(
        part_names_entry("justfile", names(&["Justfile", "justfile"])),
        PartName::Written
    );
    // A name stored decomposed (e + U+0301) against the composed one written.
    assert_eq!(
        part_names_entry("caf\u{e9}", names(&["cafe\u{301}"])),
        PartName::Unknown
    );
    assert_eq!(part_names_entry("x", names(&[])), PartName::Unknown);
}
