mod common;

use std::collections::HashSet;

use common::{folder, key_of, valid};
use sc_names::{
    COLLISION_SUFFIX_CLOSE, COLLISION_SUFFIX_OPEN, FsProfile, NameKey, Portable,
    RESERVED_DEVICE_MARKER, Reason, escape_name, portable_check,
};

const WIN: FsProfile = FsProfile::WindowsNtfs;

fn esc(name: &str, profile: FsProfile, taken: &HashSet<NameKey>) -> String {
    escape_name(&valid(name), profile, taken)
        .as_str()
        .to_owned()
}

#[test]
fn open_defaults_are_named_constants() {
    assert_eq!(RESERVED_DEVICE_MARKER, "%5F");
    assert_eq!(COLLISION_SUFFIX_OPEN, " (");
    assert_eq!(COLLISION_SUFFIX_CLOSE, ")");
}

#[test]
fn windows_escaping_is_percent_hex_and_deterministic() {
    let state = folder(&["other.txt"]);
    for _ in 0..3 {
        assert_eq!(esc("a:b", WIN, &state), "a%3Ab");
        assert_eq!(esc("CON.txt", WIN, &state), "CON%5F.txt");
    }
    let first = escape_name(&valid("a:b"), WIN, &state);
    assert_eq!(first, escape_name(&valid("a:b"), WIN, &state));
    assert_eq!(
        first.reasons(),
        &[Reason::ForbiddenChar, Reason::AlternateDataStream]
    );
    assert!(first.was_escaped());
}

#[test]
fn every_forbidden_character_and_trailing_run_is_escaped() {
    let empty = HashSet::new();
    assert_eq!(esc("<>:\"|?*\\x", WIN, &empty), "%3C%3E%3A%22%7C%3F%2A%5Cx");
    assert_eq!(esc("a. ", WIN, &empty), "a%2E%20");
    assert_eq!(esc("a.b.", WIN, &empty), "a.b%2E");
    assert_eq!(esc("a b", WIN, &empty), "a b");
    assert_eq!(esc("a.b", WIN, &empty), "a.b");
}

#[test]
fn reserved_device_names_get_the_marker_after_the_stem() {
    let empty = HashSet::new();
    for (input, expected) in [
        ("CON", "CON%5F"),
        ("con", "con%5F"),
        ("Prn.txt", "Prn%5F.txt"),
        ("AUX.tar.gz", "AUX%5F.tar.gz"),
        ("nul.", "nul%5F%2E"),
        ("COM9", "COM9%5F"),
        ("lpt1.x", "lpt1%5F.x"),
    ] {
        assert_eq!(esc(input, WIN, &empty), expected, "{input}");
    }
    assert_eq!(esc("COM0", WIN, &empty), "COM0");
    assert_eq!(
        esc("CON.txt", FsProfile::PosixCaseSensitive, &empty),
        "CON.txt"
    );
}

#[test]
fn name_that_needs_nothing_is_returned_unchanged() {
    let state = folder(&["other"]);
    for profile in [
        WIN,
        FsProfile::PosixCaseSensitive,
        FsProfile::CaseInsensitivePreserving,
    ] {
        let got = escape_name(&valid("plain name.txt"), profile, &state);
        assert_eq!(got.as_str(), "plain name.txt");
        assert!(!got.was_escaped());
        assert!(got.reasons().is_empty());
    }
}

#[test]
fn collision_appends_numbers_until_free() {
    let state = folder(&["a%3Ab", "a%3Ab (2)", "a%3Ab (3)"]);
    let got = escape_name(&valid("a:b"), WIN, &state);
    assert_eq!(got.as_str(), "a%3Ab (4)");
    assert_eq!(
        got.reasons(),
        &[
            Reason::ForbiddenChar,
            Reason::AlternateDataStream,
            Reason::CaseCollision
        ]
    );
}

#[test]
fn collision_reason_distinguishes_a_non_nfc_name() {
    let state = folder(&["caf\u{e9}"]);
    let nfd = escape_name(
        &valid("cafe\u{301}"),
        FsProfile::CaseInsensitivePreserving,
        &state,
    );
    assert_eq!(nfd.as_str(), "cafe\u{301} (2)");
    assert_eq!(nfd.reasons(), &[Reason::NfcNfdCollision]);
    let upper = escape_name(
        &valid("CAF\u{c9}"),
        FsProfile::CaseInsensitivePreserving,
        &state,
    );
    assert_eq!(upper.as_str(), "CAF\u{c9} (2)");
    assert_eq!(upper.reasons(), &[Reason::CaseCollision]);
}

#[test]
fn literal_percent_names_are_not_decoded_and_stay_distinct_through_the_collision_rule() {
    let state = folder(&["A"]);
    assert_eq!(esc("%41", WIN, &state), "%41");
    let state = folder(&["a%3Ab"]);
    assert_eq!(esc("a:b", WIN, &state), "a%3Ab (2)");
}

#[test]
fn escaped_names_are_portable_valid_and_within_255_bytes() {
    let empty = HashSet::new();
    let nasty = [
        ":".repeat(100),
        format!("{}.", "?".repeat(90)),
        format!("CON{}", ":".repeat(120)),
        format!("{} ", "\u{4e2d}".repeat(84)),
        "\u{1f600}".repeat(63),
    ];
    for name in nasty {
        let name = valid(&name);
        for profile in [
            WIN,
            FsProfile::PosixCaseSensitive,
            FsProfile::CaseInsensitivePreserving,
        ] {
            let got = escape_name(&name, profile, &empty);
            assert!(got.as_str().len() <= 255, "{} bytes", got.as_str().len());
            assert_eq!(
                portable_check(got.name(), profile),
                Portable::Ok,
                "{:?}",
                got.as_str()
            );
        }
    }
}

#[test]
fn truncation_leaves_room_for_the_collision_suffix() {
    let long = valid(&":".repeat(100));
    let mut taken = HashSet::new();
    for round in 0..5 {
        let got = escape_name(&long, WIN, &taken);
        assert!(got.as_str().len() <= 255);
        assert!(taken.insert(key_of(got.as_str())), "round {round}");
    }
}

#[test]
fn name_over_the_decomposed_limit_is_reported_and_cut_on_case_insensitive_roots() {
    let name = valid(&"\u{390}".repeat(100)); // 200 bytes, 300 UTF-16 units once decomposed
    assert_eq!(
        portable_check(&name, FsProfile::CaseInsensitivePreserving),
        Portable::NeedsEscape(vec![Reason::OverPlatformLimit])
    );
    assert_eq!(portable_check(&name, WIN), Portable::Ok);
    let got = escape_name(&name, FsProfile::CaseInsensitivePreserving, &HashSet::new());
    assert_eq!(got.reasons(), &[Reason::OverPlatformLimit]);
    assert_eq!(
        portable_check(got.name(), FsProfile::CaseInsensitivePreserving),
        Portable::Ok
    );
    assert!(got.as_str().chars().all(|c| c == '\u{390}'));
    assert_eq!(got.as_str().chars().count(), 85);
}
