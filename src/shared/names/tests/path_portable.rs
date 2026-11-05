mod common;

use common::valid;
use sc_names::{FsProfile, NameError, Portable, Reason, portable_check, split_path};

const WIN: FsProfile = FsProfile::WindowsNtfs;
const POSIX: FsProfile = FsProfile::PosixCaseSensitive;
const CI: FsProfile = FsProfile::CaseInsensitivePreserving;

fn reasons(name: &str, profile: FsProfile) -> Vec<Reason> {
    match portable_check(&valid(name), profile) {
        Portable::Ok => Vec::new(),
        Portable::NeedsEscape(r) => r,
    }
}

#[test]
fn split_path_splits_on_slash_only() {
    let parts: Vec<String> = split_path("docs/a b/c\\d.txt")
        .expect("legal")
        .into_iter()
        .map(|n| n.into_string())
        .collect();
    assert_eq!(parts, ["docs", "a b", "c\\d.txt"]);
    assert_eq!(split_path("single").expect("legal").len(), 1);
}

#[test]
fn split_path_rejects_traversal_and_empty_components() {
    assert_eq!(split_path("a/../b"), Err(NameError::DotDot));
    assert_eq!(split_path("./a"), Err(NameError::Dot));
    assert_eq!(split_path(".."), Err(NameError::DotDot));
    assert_eq!(split_path(""), Err(NameError::Empty));
    assert_eq!(split_path("/a"), Err(NameError::Empty));
    assert_eq!(split_path("a/"), Err(NameError::Empty));
    assert_eq!(split_path("a//b"), Err(NameError::Empty));
    assert_eq!(split_path("a/b\0c"), Err(NameError::ContainsNul));
    assert_eq!(
        split_path(&format!("a/{}", "x".repeat(256))),
        Err(NameError::TooLong(256))
    );
    assert_eq!(split_path("a/\u{202e}b"), Err(NameError::BidiControl));
}

#[test]
fn reserved_windows_device_names_with_and_without_extension() {
    let reserved = [
        "CON",
        "prn",
        "Aux",
        "NUL",
        "COM1",
        "com9",
        "LPT1",
        "lpt9",
        "CON.txt",
        "nul.tar.gz",
        "COM\u{b9}",
        "LPT\u{b2}.x",
        "COM\u{b3}",
        "CON .txt",
        "con ",
    ];
    for name in reserved {
        assert!(
            reasons(name, WIN).contains(&Reason::ReservedDeviceName),
            "{name:?}"
        );
    }
    for name in [
        "COM0",
        "COM10",
        "LPT",
        "CONSOLE",
        "CONN",
        "xCON",
        "a.CON",
        "COM1x",
        "COM\u{b9}\u{b9}",
        "NULL",
    ] {
        assert!(
            !reasons(name, WIN).contains(&Reason::ReservedDeviceName),
            "{name:?}"
        );
    }
}

#[test]
fn windows_reasons_are_reported_once_each_in_a_fixed_order() {
    assert_eq!(
        reasons("CON.", WIN),
        [Reason::ReservedDeviceName, Reason::TrailingDotOrSpace]
    );
    assert_eq!(
        reasons("a:b:c", WIN),
        [Reason::ForbiddenChar, Reason::AlternateDataStream]
    );
    assert_eq!(reasons("a<b", WIN), [Reason::ForbiddenChar]);
    assert_eq!(reasons("a ", WIN), [Reason::TrailingDotOrSpace]);
    assert_eq!(reasons("a.", WIN), [Reason::TrailingDotOrSpace]);
    for c in ['<', '>', ':', '"', '|', '?', '*', '\\'] {
        assert!(
            !portable_check(&valid(&format!("a{c}b")), WIN).is_ok(),
            "{c}"
        );
    }
}

#[test]
fn only_windows_has_character_and_device_rules() {
    for name in ["CON", "a.", "a ", "a:b", "a<b", "a\\b"] {
        assert_eq!(
            portable_check(&valid(name), POSIX),
            Portable::Ok,
            "{name:?}"
        );
        assert_eq!(portable_check(&valid(name), CI), Portable::Ok, "{name:?}");
    }
}

#[test]
fn lone_surrogates_cannot_occur_in_a_rust_name() {
    // A `&str` is valid UTF-8; a surrogate code point is not a `char`, so `portable_check` never reports it.
    assert!(char::from_u32(0xD800).is_none());
    for name in ["\u{fffd}", "a\u{ffff}b"] {
        assert!(!reasons(name, WIN).contains(&Reason::LoneSurrogate));
    }
}
