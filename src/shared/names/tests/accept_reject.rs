mod common;

use sc_names::{MAX_NAME_BYTES, NameError, validate_name};

#[test]
fn rejects_exactly_the_listed_classes() {
    let cases: &[(&str, NameError)] = &[
        ("", NameError::Empty),
        (".", NameError::Dot),
        ("..", NameError::DotDot),
        ("a/b", NameError::ContainsSlash),
        ("/", NameError::ContainsSlash),
        ("a\0b", NameError::ContainsNul),
        ("\0", NameError::ContainsNul),
    ];
    for (input, expected) in cases {
        assert_eq!(validate_name(input).unwrap_err(), *expected, "{input:?}");
    }
}

#[test]
fn every_cc_control_character_is_rejected() {
    let cc = (0x01u32..=0x1F).chain(0x7F..=0x9F);
    let mut count = 0;
    for code in cc {
        let c = char::from_u32(code).expect("Cc code points are scalar values");
        for name in [format!("a{c}b"), c.to_string(), format!("{c}x")] {
            assert_eq!(
                validate_name(&name),
                Err(NameError::ControlChar),
                "U+{code:04X}"
            );
        }
        count += 1;
    }
    assert_eq!(count, 31 + 33, "C0 without NUL plus DEL and C1");
}

#[test]
fn every_listed_bidi_control_is_rejected() {
    let bidi = [
        0x061Cu32, 0x200E, 0x200F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E, 0x2066, 0x2067, 0x2068,
        0x2069,
    ];
    for code in bidi {
        let c = char::from_u32(code).expect("scalar value");
        for name in [format!("a{c}b"), c.to_string(), format!("file{c}")] {
            assert_eq!(
                validate_name(&name),
                Err(NameError::BidiControl),
                "U+{code:04X}"
            );
        }
    }
}

#[test]
fn length_limit_is_255_bytes_not_characters() {
    assert_eq!(MAX_NAME_BYTES, 255);
    assert!(validate_name(&"a".repeat(255)).is_ok());
    assert_eq!(
        validate_name(&"a".repeat(256)),
        Err(NameError::TooLong(256))
    );
    let two_byte = "\u{e9}".repeat(127);
    assert_eq!(two_byte.len(), 254);
    assert!(validate_name(&two_byte).is_ok());
    assert_eq!(
        validate_name(&"\u{e9}".repeat(128)),
        Err(NameError::TooLong(256))
    );
    assert!(validate_name(&format!("{}\u{20ac}", "a".repeat(252))).is_ok());
    assert_eq!(
        validate_name(&format!("{}\u{20ac}", "a".repeat(253))),
        Err(NameError::TooLong(256))
    );
}

#[test]
fn accepts_everything_else() {
    let accepted = [
        "CON",
        "con.txt",
        "NUL",
        "COM1",
        "a.",
        "a ",
        " a",
        "a\\b",
        "a:b",
        "-x",
        "--help",
        "\u{1F600}",
        "Readme.md",
        "README.MD",
        "...",
        ".hidden",
        "a..",
        "..a",
        "a b",
        "%41",
        "a\u{a0}b",
        "a\u{200b}b",
        "\u{feff}",
        "a\u{2028}b",
        "\u{2060}x",
        "\u{206a}x",
        "\u{2065}",
        "\u{202f}x",
        "CON.",
        "a:b:c",
        "\u{0301}",
        "caf\u{e9}",
        "cafe\u{301}",
        "\u{ffff}",
        "\u{10ffff}",
    ];
    for name in accepted {
        let valid =
            validate_name(name).unwrap_or_else(|e| panic!("{name:?} must be accepted: {e}"));
        assert_eq!(valid.as_str(), name, "the validated name keeps its text");
    }
}

#[test]
fn check_order_is_length_then_dots_then_first_bad_character() {
    assert_eq!(
        validate_name(&format!("{}/", "a".repeat(255))),
        Err(NameError::TooLong(256))
    );
    assert_eq!(validate_name("a/\0"), Err(NameError::ContainsSlash));
    assert_eq!(validate_name("\0/"), Err(NameError::ContainsNul));
    assert_eq!(validate_name("\u{202e}\t"), Err(NameError::BidiControl));
}

#[test]
fn error_messages_name_the_class() {
    assert_eq!(
        NameError::TooLong(300).to_string(),
        "name is 300 bytes, the limit is 255"
    );
    assert_eq!(NameError::DotDot.to_string(), "name is `..`");
}
