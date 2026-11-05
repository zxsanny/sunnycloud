mod common;

use std::collections::HashSet;

use common::{folder, key_of, valid};
use sc_clock::Timestamp;
use sc_names::{NameKey, conflict_copy_name, name_key, validate_name};

// 2026-11-05 14:30:59 UTC
const AT: Timestamp = Timestamp::from_unix_micros(1_793_889_059_000_000);

fn copy(name: &str, label: &str, taken: &HashSet<NameKey>) -> String {
    conflict_copy_name(&valid(name), AT, label, taken).into_string()
}

#[test]
fn format_is_stem_conflict_copy_utc_minute_device_extension() {
    assert_eq!(
        copy("report.docx", "laptop", &HashSet::new()),
        "report (conflict copy 2026-11-05 14-30 laptop).docx"
    );
}

#[test]
fn extension_is_text_from_the_last_dot_when_there_is_a_stem() {
    let none = HashSet::new();
    assert_eq!(
        copy("a.tar.gz", "d", &none),
        "a.tar (conflict copy 2026-11-05 14-30 d).gz"
    );
    assert_eq!(
        copy("noext", "d", &none),
        "noext (conflict copy 2026-11-05 14-30 d)"
    );
    assert_eq!(
        copy(".bashrc", "d", &none),
        ".bashrc (conflict copy 2026-11-05 14-30 d)"
    );
    assert_eq!(
        copy("a.", "d", &none),
        "a (conflict copy 2026-11-05 14-30 d)."
    );
    assert_eq!(
        copy("\u{e9}.t\u{e9}xt", "d", &none),
        "\u{e9} (conflict copy 2026-11-05 14-30 d).t\u{e9}xt"
    );
}

#[test]
fn time_is_utc_and_floored_to_the_minute() {
    let none = HashSet::new();
    let at = |micros| {
        conflict_copy_name(&valid("f"), Timestamp::from_unix_micros(micros), "", &none)
            .into_string()
    };
    assert_eq!(at(0), "f (conflict copy 1970-01-01 00-00)");
    assert_eq!(at(59_999_999), "f (conflict copy 1970-01-01 00-00)");
    assert_eq!(at(60_000_000), "f (conflict copy 1970-01-01 00-01)");
    assert_eq!(at(-1), "f (conflict copy 1969-12-31 23-59)");
    assert_eq!(
        at(951_782_400_000_000),
        "f (conflict copy 2000-02-29 00-00)"
    );
}

#[test]
fn label_is_sanitised_with_underscores() {
    let label = "my/laptop\0\u{7}\u{202e}\u{61c}\u{2069}\u{85}x";
    assert_eq!(
        copy("a.txt", label, &HashSet::new()),
        "a (conflict copy 2026-11-05 14-30 my_laptop______x).txt"
    );
}

#[test]
fn empty_label_drops_the_label_part() {
    assert_eq!(
        copy("a.txt", "", &HashSet::new()),
        "a (conflict copy 2026-11-05 14-30).txt"
    );
}

#[test]
fn long_multibyte_stem_and_long_label_fit_255_bytes_and_keep_the_extension() {
    let stem = "\u{4e2d}".repeat(83); // 249 bytes
    let name = format!("{stem}.txt");
    assert!(
        validate_name(&name).is_ok(),
        "the contested name is a legal 253-byte name"
    );
    let label = format!(
        "{}/\u{202e}{}",
        "d\u{e9}vice".repeat(30),
        "\u{1f600}".repeat(20)
    );
    let got = copy(&name, &label, &HashSet::new());
    assert!(got.len() <= 255, "{} bytes", got.len());
    assert!(got.ends_with(").txt"));
    assert!(got.contains("(conflict copy 2026-11-05 14-30"));
    assert!(!got.contains('/') && !got.contains('\u{202e}'));
    assert!(validate_name(&got).is_ok());
    assert!(
        got.starts_with(&"\u{4e2d}".repeat(10)),
        "the stem is kept before the label is cut"
    );
}

#[test]
fn label_is_cut_before_the_stem() {
    let name = format!("{}.txt", "a".repeat(100));
    let got = copy(&name, &"L".repeat(300), &HashSet::new());
    assert_eq!(got.len(), 255);
    assert!(
        got.starts_with(&format!("{} (conflict copy", "a".repeat(100))),
        "full stem kept"
    );
    assert!(got.ends_with("L).txt"));

    let long_stem = format!("{}.txt", "a".repeat(250));
    let got = copy(&long_stem, "device", &HashSet::new());
    assert!(got.len() <= 255);
    assert!(
        !got.contains("device"),
        "label dropped before the stem is cut: {got}"
    );
    assert!(got.ends_with("14-30).txt"));
    assert!(got.starts_with(&"a".repeat(150)));
}

#[test]
fn truncation_never_splits_a_character() {
    for extra in 0..4 {
        let name = format!("{}{}.txt", "a".repeat(extra), "\u{1f600}".repeat(60));
        let got = copy(&name, "x", &HashSet::new());
        assert!(got.len() <= 255);
        assert!(validate_name(&got).is_ok());
    }
}

#[test]
fn oversized_extension_is_part_of_the_stem() {
    let name = format!("a.{}", "x".repeat(200));
    let got = copy(&name, "d", &HashSet::new());
    assert!(got.len() <= 255);
    assert!(!got.ends_with(&"x".repeat(65)));
    assert!(got.contains("(conflict copy"));
    assert!(validate_name(&got).is_ok());

    let at_cap = format!("a.{}", "x".repeat(sc_names::MAX_EXTENSION_BYTES - 1));
    assert!(copy(&at_cap, "d", &HashSet::new()).ends_with(&format!("d).{}", "x".repeat(63))));
}

#[test]
fn taken_result_gets_a_numeric_suffix_before_the_extension() {
    let first = copy("report.docx", "laptop", &HashSet::new());
    let mut taken = folder(&[&first]);
    let second = copy("report.docx", "laptop", &taken);
    assert_eq!(
        second,
        "report (conflict copy 2026-11-05 14-30 laptop) (2).docx"
    );
    taken.insert(key_of(&second));
    let third = copy("report.docx", "laptop", &taken);
    assert_eq!(
        third,
        "report (conflict copy 2026-11-05 14-30 laptop) (3).docx"
    );
}

#[test]
fn taken_check_uses_the_case_insensitive_key() {
    let taken = folder(&["REPORT (CONFLICT COPY 2026-11-05 14-30 LAPTOP).DOCX"]);
    assert_eq!(
        copy("report.docx", "laptop", &taken),
        "report (conflict copy 2026-11-05 14-30 laptop) (2).docx"
    );
}

#[test]
fn suffixed_names_still_fit_255_bytes() {
    let name = format!("{}.txt", "\u{4e2d}".repeat(80));
    let mut taken: HashSet<NameKey> = HashSet::new();
    for round in 0..120 {
        let got = conflict_copy_name(&valid(&name), AT, &"d".repeat(300), &taken);
        assert!(got.as_str().len() <= 255, "round {round}");
        assert!(got.as_str().ends_with(".txt"));
        assert!(
            taken.insert(name_key(&got)),
            "result must be new in round {round}"
        );
    }
}
