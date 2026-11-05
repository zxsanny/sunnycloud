mod common;

use common::{key_of, valid};
use sc_names::{NAME_KEY_UNICODE_VERSION, NAME_KEY_VERSION, name_key, validate_name};

#[test]
fn case_variants_share_one_key_and_the_name_keeps_its_case() {
    let lower = valid("Readme.md");
    let upper = valid("README.MD");
    assert_eq!(name_key(&lower), name_key(&upper));
    assert_eq!(lower.as_str(), "Readme.md");
    assert_eq!(upper.as_str(), "README.MD");
    assert_eq!(name_key(&lower).as_str(), "readme.md");
}

#[test]
fn nfc_and_nfd_spellings_share_one_key() {
    let nfc = valid("caf\u{e9}.txt");
    let nfd = valid("cafe\u{301}.txt");
    assert_ne!(nfc.as_str(), nfd.as_str());
    assert_eq!(name_key(&nfc), name_key(&nfd));
    assert_eq!(
        key_of("\u{c5}ngstr\u{f6}m"),
        key_of("A\u{30a}ngstro\u{308}m")
    );
    assert_eq!(key_of("\u{ac00}"), key_of("\u{1100}\u{1161}"));
}

#[test]
fn full_case_folding_not_simple_lowercasing() {
    assert_eq!(key_of("Stra\u{df}e").as_str(), "strasse");
    assert_eq!(key_of("STRASSE"), key_of("Stra\u{df}e"));
    assert_eq!(key_of("\u{1e9e}").as_str(), "ss");
    assert_eq!(key_of("\u{fb01}").as_str(), "fi");
    assert_eq!(key_of("\u{3a3}"), key_of("\u{3c2}"));
    assert_eq!(key_of("\u{3c3}"), key_of("\u{3c2}"));
    assert_eq!(key_of("\u{10400}"), key_of("\u{10428}"));
    assert_eq!(key_of("\u{1e900}"), key_of("\u{1e922}"));
    assert_eq!(key_of("\u{3a9}"), key_of("\u{2126}"));
}

#[test]
fn turkic_dotted_and_dotless_i_follow_the_default_locale_independent_fold() {
    assert_eq!(key_of("I"), key_of("i"));
    assert_eq!(key_of("\u{130}").as_str(), "i\u{307}");
    assert_ne!(key_of("\u{131}"), key_of("i"), "dotless i stays distinct");
    assert_ne!(key_of("\u{131}"), key_of("I"));
    assert_eq!(key_of("\u{131}").as_str(), "\u{131}");
    assert_eq!(key_of("\u{130}"), key_of("i\u{307}"));
}

#[test]
fn combining_marks_after_folding_are_recomposed() {
    assert_eq!(key_of("\u{df}\u{301}").as_str(), "s\u{15b}");
    assert_eq!(
        name_key(&validate_name(key_of("\u{df}\u{301}").as_str()).unwrap()).as_str(),
        "s\u{15b}"
    );
}

#[test]
fn key_is_stable_across_calls_and_byte_exact() {
    let name = valid("Mixed CASE \u{e9}\u{df}.TXT");
    let first = name_key(&name);
    for _ in 0..100 {
        assert_eq!(name_key(&name), first);
    }
    assert_eq!(first.as_bytes(), first.as_str().as_bytes());
    assert_ne!(key_of("a"), key_of("a "), "no trimming");
    assert_ne!(key_of("a.txt"), key_of("a.txt."), "no trimming");
}

#[test]
fn version_constants_report_the_pinned_unicode_version() {
    assert_eq!(NAME_KEY_UNICODE_VERSION, "18.0.0");
    assert_eq!(NAME_KEY_VERSION, 1800);
}

fn all_legal_scalars() -> impl Iterator<Item = char> {
    (0..=0x0010_FFFFu32)
        .filter_map(char::from_u32)
        .filter(|c| validate_name(&c.to_string()).is_ok())
}

/// FNV-1a over the key of every legal scalar value. A library bump that changes any fold or
/// normalisation result changes this digest: then bump `NAME_KEY_VERSION`, `NAME_KEY_UNICODE_VERSION`
/// and the two pins below together (stored keys change, so a batch re-key is needed).
#[test]
fn fold_table_digest_pins_the_unicode_version() {
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut scalars = 0usize;
    for c in all_legal_scalars() {
        scalars += 1;
        for byte in key_of(&c.to_string()).as_bytes().iter().chain(&[0xFF]) {
            digest ^= u64::from(*byte);
            digest = digest.wrapping_mul(0x0100_0000_01b3);
        }
    }
    // 1_112_064 scalar values minus 65 Cc, `/`, `.` (a name of its own) and 12 bidi controls.
    assert_eq!(scalars, 1_112_064 - 65 - 1 - 1 - 12);
    assert_eq!(
        digest, 0x5992_aece_0fe3_4ace,
        "fold table changed: bump NAME_KEY_VERSION"
    );
}

#[test]
fn key_is_idempotent_for_every_scalar() {
    for c in all_legal_scalars() {
        let key = key_of(&c.to_string());
        let again = name_key(&validate_name(key.as_str()).expect("a key is a legal name"));
        assert_eq!(again, key, "U+{:04X}", u32::from(c));
    }
}

#[test]
fn key_is_idempotent_for_cased_scalars_with_combining_marks() {
    let cased: Vec<char> = all_legal_scalars()
        .filter(|c| key_of(&c.to_string()).as_str() != c.to_string())
        .collect();
    let marks = [
        '\u{300}', '\u{301}', '\u{308}', '\u{323}', '\u{345}', '\u{35c}', '\u{1dc0}', '\u{20d0}',
    ];
    for &c in &cased {
        for m in marks {
            for text in [
                format!("{c}{m}"),
                format!("{m}{c}"),
                format!("{c}\u{345}{m}"),
                format!("{c}{m}\u{345}"),
            ] {
                let key = key_of(&text);
                let again = name_key(&validate_name(key.as_str()).expect("a key is a legal name"));
                assert_eq!(again, key, "{text:?}");
            }
        }
    }
}
