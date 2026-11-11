//! Tests of [`Comparison`]: what each counts of a file, and the words that name one.

use super::*;

fn lines(ignore: &[&str]) -> Comparison {
    Comparison::Lines {
        ignore: ignore.iter().map(|i| LinePrefix::new(i).unwrap()).collect(),
    }
}

fn same(comparison: &Comparison, a: &str, b: &str) -> bool {
    comparison.fingerprint(a.as_bytes()) == comparison.fingerprint(b.as_bytes())
}

const CODE: &str = "fn main() {\n    let x = 1;\n}\n";

#[test]
fn bytes_is_the_hash_of_the_content() {
    for bytes in [&b""[..], b"x", CODE.as_bytes(), &[0xff, 0x00]] {
        assert_eq!(
            Comparison::Bytes.fingerprint(bytes),
            ArtifactHash::of_content(bytes)
        );
    }
    assert_eq!(Comparison::default(), Comparison::Bytes);
    assert!(!same(
        &Comparison::Bytes,
        CODE,
        "fn main() {\n  let x = 1;\n}\n"
    ));
}

#[test]
fn lines_does_not_see_white_space_indentation_blank_lines_or_line_ends() {
    let c = lines(&[]);
    for other in [
        "fn main() {\n  let x = 1;\n}\n",
        "fn   main()   {\n\tlet x  =  1;   \n}",
        "\n\nfn main() {\n\n    let x = 1;\n   \n}\n\n",
        "fn main() {\r\n    let x = 1;\r\n}\r\n",
    ] {
        assert!(same(&c, CODE, other), "{other:?}");
    }
}

#[test]
fn lines_leaves_out_the_lines_an_ignored_prefix_starts_after_white_space() {
    let c = lines(&["//"]);
    for other in [
        "// a comment\nfn main() {\n    let x = 1;\n}\n",
        "/// doc\nfn main() {\n    let x = 1;\n}\n",
        "//! module\nfn main() {\n    // inside\n    let x = 1;\n}\n",
        "fn main() {\n        // indented\n    let x = 1;\n}\n",
    ] {
        assert!(same(&c, CODE, other), "{other:?}");
    }
    // Without the prefix, a comment is a line like any other.
    assert!(!same(&lines(&[]), CODE, &format!("// a comment\n{CODE}")));
}

#[test]
fn lines_sees_a_line_of_code_and_a_comment_after_code() {
    let c = lines(&["//"]);
    assert!(!same(&c, CODE, "fn main() {\n    let x = 2;\n}\n"));
    assert!(!same(&c, CODE, "fn main() {\n    let x = 1; // one\n}\n"));
    // Two words joined is not the same as two words apart.
    assert!(!same(&c, "let x = 1;\n", "letx = 1;\n"));
    // The order of the lines counts.
    assert!(!same(&c, "a\nb\n", "b\na\n"));
}

#[test]
fn hashes_made_with_other_prefixes_or_of_the_bytes_never_meet() {
    let text = "a\n".as_bytes();
    let plain = lines(&[]).fingerprint(text);
    let slashes = lines(&["//"]).fingerprint(text);
    let hashes = lines(&["#"]).fingerprint(text);
    assert_ne!(plain, slashes);
    assert_ne!(slashes, hashes);
    // The prefixes are a set: their order as written does not count.
    assert_eq!(
        lines(&["//", "#"]).fingerprint(text),
        lines(&["#", "//"]).fingerprint(text)
    );
    for comparison in [lines(&[]), lines(&["//"])] {
        for bytes in [&b""[..], b"a", b"a\n", CODE.as_bytes()] {
            assert_ne!(
                comparison.fingerprint(bytes),
                Comparison::Bytes.fingerprint(bytes),
                "{comparison:?} {bytes:?}"
            );
        }
    }
}

#[test]
fn bytes_that_are_not_utf8_have_one_lines_hash_and_each_byte_counts() {
    let c = lines(&["//"]);
    let bytes = [b'a', 0xff, 0xfe, b'\n', b'b'];
    assert_eq!(c.fingerprint(&bytes), c.fingerprint(&bytes));
    // Each byte that is not UTF-8 is kept as itself: two such bytes are two texts.
    assert_ne!(c.fingerprint(b"caf\xe9\n"), c.fingerprint(b"caf\xe8\n"));
    assert_ne!(
        c.fingerprint(&bytes),
        c.fingerprint(&[b'a', 0xfe, 0xff, b'\n', b'b'])
    );
    assert_ne!(c.fingerprint(&bytes), c.fingerprint(b"a\nb"));
    // White space around such a byte still does not count.
    assert_eq!(
        c.fingerprint(b"caf\xe9\n"),
        c.fingerprint(b"   caf\xe9  \r\n")
    );
}

/// An invalid byte is written as `\xNN`, and a `\` of the text as `\\`, so the text
/// `\xNN` in a file never reads as the byte it names.
#[test]
fn an_invalid_byte_is_not_the_text_that_names_it() {
    let c = lines(&[]);
    assert_ne!(c.fingerprint(b"caf\xe9\n"), c.fingerprint(b"caf\\xe9\n"));
}

#[test]
fn a_lone_carriage_return_ends_a_line() {
    let c = lines(&["//"]);
    // An old Mac file: a header comment, then code, on `\r`-separated lines.
    let mac = b"// header\rfn main() {\r    let x = 1;\r}\r";
    assert!(same(&c, CODE, std::str::from_utf8(mac).unwrap()));
    // The code is not dropped with the comment it follows.
    assert_ne!(c.fingerprint(mac), c.fingerprint(b"// header\r"));
    assert_ne!(
        c.fingerprint(mac),
        c.fingerprint(b"// header\rfn main() {\r    let x = 2;\r}\r")
    );
    // `\r\n`, `\n` and `\r` end a line alike.
    assert!(same(&c, "a\r\nb\n", "a\rb\r"));
}

#[test]
fn a_comparison_is_read_from_its_words_and_writes_them_back() {
    assert_eq!(Comparison::parse("bytes", &[]), Ok(Comparison::Bytes));
    let read = Comparison::parse("lines", &["//".to_owned(), "#".to_owned()]).unwrap();
    assert_eq!(read, lines(&["#", "//"]));
    assert_eq!(read.kind_word(), "lines");
    assert_eq!(
        read.ignored().map(LinePrefix::as_str).collect::<Vec<_>>(),
        ["#", "//"]
    );
    assert_eq!(Comparison::parse("lines", &[]), Ok(lines(&[])));
    assert_eq!(Comparison::Bytes.kind_word(), "bytes");
    assert_eq!(Comparison::Bytes.ignored().count(), 0);
}

#[test]
fn a_comparison_that_is_none_is_refused() {
    for kind in ["Lines", "text", " lines"] {
        assert_eq!(
            Comparison::parse(kind, &[]),
            Err(ComparisonProblem::UnknownKind {
                found: kind.to_owned()
            })
        );
    }
    // No kind: `ignore` written without `compare`.
    for ignore in [vec![], vec!["//".to_owned()]] {
        assert_eq!(
            Comparison::parse("", &ignore),
            Err(ComparisonProblem::NoKind)
        );
    }
    assert_eq!(
        ComparisonProblem::NoKind.to_string(),
        "`ignore` needs `compare = \"lines\"`"
    );
    assert_eq!(
        Comparison::parse("bytes", &["//".to_owned()]),
        Err(ComparisonProblem::IgnoreWithBytes)
    );
    for prefix in ["", "// ", "a b", "\t#", "x\u{7}"] {
        assert_eq!(
            Comparison::parse("lines", &[prefix.to_owned()]),
            Err(ComparisonProblem::BadPrefix {
                found: prefix.to_owned()
            }),
            "{prefix:?}"
        );
        assert!(LinePrefix::new(prefix).is_err(), "{prefix:?}");
    }
    assert_eq!(LinePrefix::new("--").unwrap().as_str(), "--");
}

#[test]
fn presence_ignores_the_content_and_tells_present_from_absent() {
    let c = Comparison::Presence;
    for other in ["", "x", "fn other() {}\n"] {
        assert!(same(&c, CODE, other), "{other:?}");
    }
    assert_eq!(c.fingerprint(&[0xff]), c.fingerprint(b""));
    // Its hash is of a kind of its own: never the hash of some bytes or lines.
    assert_ne!(c.fingerprint(b""), Comparison::Bytes.fingerprint(b""));
    assert_ne!(c.fingerprint(b""), lines(&[]).fingerprint(b""));
    // A thing there has the hash; a thing not there has none.
    let asked = BTreeSet::from([Comparison::Presence]);
    let there = ArtifactReading::of_bytes(CODE.as_bytes(), &asked, None);
    let edited = ArtifactReading::of_bytes(b"else", &asked, None);
    assert_eq!(there.state(&c), edited.state(&c));
    assert_eq!(there.state(&c), ArtifactState::Present(c.fingerprint(b"")));
    assert_ne!(ArtifactReading::absent().state(&c), there.state(&c));
    // Its words.
    assert_eq!(Comparison::parse("presence", &[]), Ok(Comparison::Presence));
    assert_eq!(c.kind_word(), "presence");
    assert_eq!(c.ignored().count(), 0);
    assert_eq!(
        Comparison::parse("presence", &["//".to_owned()]),
        Err(ComparisonProblem::IgnoreWithPresence)
    );
}

#[test]
fn a_thing_there_with_no_content_read_has_the_presence_hash_only() {
    let why = NonBlank::new("only presence is compared here").unwrap();
    let there = ArtifactReading::there(why.clone());
    assert!(there.is_present());
    assert_eq!(there.address(), None);
    let asked = BTreeSet::from([Comparison::Presence]);
    assert_eq!(
        there.state(&Comparison::Presence),
        ArtifactReading::of_bytes(CODE.as_bytes(), &asked, None).state(&Comparison::Presence)
    );
    // No bytes were read, so no comparison of bytes or lines can be told of it, for
    // the reason the adapter gave.
    for other in [Comparison::Bytes, lines(&[])] {
        assert_eq!(
            there.state(&other),
            ArtifactState::Unknown(Untold::Said(why.clone())),
            "{other:?}"
        );
    }
}

#[test]
fn a_reading_has_the_hash_of_each_comparison_asked_and_of_its_bytes() {
    let asked = BTreeSet::from([lines(&["//"])]);
    let reading = ArtifactReading::of_bytes(CODE.as_bytes(), &asked, None);
    assert_eq!(
        reading.state(&lines(&["//"])),
        ArtifactState::Present(lines(&["//"]).fingerprint(CODE.as_bytes()))
    );
    assert_eq!(
        reading.state(&Comparison::Bytes),
        ArtifactState::Present(ArtifactHash::of_content(CODE.as_bytes()))
    );
    // A comparison not asked cannot be told: it is never read as nothing there.
    assert_eq!(
        reading.state(&lines(&[])),
        ArtifactState::Unknown(Untold::NotAsked)
    );
}
