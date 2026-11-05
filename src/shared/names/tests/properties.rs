mod common;

use std::collections::HashSet;

use common::{Rng, random_string, random_valid_name};
use sc_clock::Timestamp;
use sc_names::{
    FsProfile, NameKey, Portable, conflict_copy_name, escape_name, name_key, portable_check,
    split_path, validate_name,
};

const ROUNDS: usize = 3000;
const PROFILES: [FsProfile; 3] = [
    FsProfile::WindowsNtfs,
    FsProfile::PosixCaseSensitive,
    FsProfile::CaseInsensitivePreserving,
];

fn random_timestamp(rng: &mut Rng) -> Timestamp {
    let micros = match rng.below(3) {
        0 => i64::try_from(rng.next_u64() >> 1).expect("63 bits fit"),
        1 => -i64::try_from(rng.next_u64() >> 1).expect("63 bits fit"),
        _ => 1_793_889_059_000_000 + i64::try_from(rng.below(1 << 30)).expect("fits"),
    };
    Timestamp::from_unix_micros(micros)
}

#[test]
fn conflict_copy_names_always_validate() {
    let mut rng = Rng::new(0xC0FF_EE01);
    for round in 0..ROUNDS {
        let contested = random_valid_name(&mut rng, 120);
        let label = random_string(&mut rng, 140);
        let mut taken: HashSet<NameKey> = HashSet::new();
        for _ in 0..rng.below(4) {
            let earlier =
                conflict_copy_name(&contested, random_timestamp(&mut rng), &label, &taken);
            taken.insert(name_key(&earlier));
        }
        let at = random_timestamp(&mut rng);
        let copy = conflict_copy_name(&contested, at, &label, &taken);
        assert!(
            validate_name(copy.as_str()).is_ok(),
            "round {round}: {copy:?}"
        );
        assert!(copy.as_str().len() <= 255, "round {round}");
        assert!(
            !taken.contains(&name_key(&copy)),
            "round {round}: result is already taken"
        );
        assert!(copy.as_str().contains("(conflict copy "), "round {round}");
    }
}

#[test]
fn conflict_copy_keeps_a_short_extension() {
    let mut rng = Rng::new(0xC0FF_EE02);
    for round in 0..ROUNDS {
        let stem = random_valid_name(&mut rng, 100);
        let ext: String = (0..1 + rng.below(8))
            .map(|_| rng.pick(&['t', 'x', 'T', 'z', '1', '\u{e9}']))
            .collect();
        let Ok(contested) = validate_name(&format!("{}.{ext}", stem.as_str())) else {
            continue;
        };
        let copy = conflict_copy_name(
            &contested,
            random_timestamp(&mut rng),
            &random_string(&mut rng, 200),
            &HashSet::new(),
        );
        assert!(
            copy.as_str().ends_with(&format!(".{ext}")),
            "round {round}: {copy:?}"
        );
        assert!(copy.as_str().len() <= 255);
    }
}

#[test]
fn name_key_is_idempotent() {
    let mut rng = Rng::new(0xC0FF_EE03);
    for round in 0..ROUNDS {
        let name = random_valid_name(&mut rng, 60);
        let key = name_key(&name);
        let Ok(again) = validate_name(key.as_str()) else {
            continue;
        };
        assert_eq!(name_key(&again), key, "round {round}: {name:?}");
    }
}

#[test]
fn name_key_ignores_case() {
    let mut rng = Rng::new(0xC0FF_EE04);
    for round in 0..ROUNDS {
        let name = random_valid_name(&mut rng, 60);
        // The default fold keeps dotless i apart from i, and std's upper-casing of U+0345 changes its
        // combining class; neither is a case difference the fold is meant to equate.
        if name.as_str().contains(['\u{131}', '\u{130}', '\u{345}']) {
            continue;
        }
        let upper = name.as_str().to_uppercase();
        let lower = name.as_str().to_lowercase();
        let (Ok(upper), Ok(lower)) = (validate_name(&upper), validate_name(&lower)) else {
            continue;
        };
        assert_eq!(
            name_key(&upper),
            name_key(&lower),
            "round {round}: {name:?}"
        );
        assert_eq!(name_key(&upper), name_key(&name), "round {round}: {name:?}");
    }
}

#[test]
fn escaped_names_are_valid_portable_free_and_deterministic() {
    let mut rng = Rng::new(0xC0FF_EE05);
    for round in 0..ROUNDS {
        let name = random_valid_name(&mut rng, 120);
        let mut taken: HashSet<NameKey> = HashSet::new();
        for _ in 0..rng.below(5) {
            let sibling = random_valid_name(&mut rng, 12);
            taken.insert(name_key(&sibling));
        }
        let first_try = escape_name(&name, FsProfile::WindowsNtfs, &taken);
        taken.insert(name_key(first_try.name()));
        for profile in PROFILES {
            let got = escape_name(&name, profile, &taken);
            assert_eq!(
                got,
                escape_name(&name, profile, &taken),
                "round {round}: deterministic"
            );
            assert!(validate_name(got.as_str()).is_ok(), "round {round}");
            assert!(
                !taken.contains(&name_key(got.name())),
                "round {round}: {:?} collides",
                got.as_str()
            );
            assert_eq!(
                portable_check(got.name(), profile),
                Portable::Ok,
                "round {round}: {:?}",
                got.as_str()
            );
            if !got.was_escaped() {
                assert_eq!(got.as_str(), name.as_str());
            }
        }
    }
}

#[test]
fn split_path_never_yields_dots_or_separators() {
    let mut rng = Rng::new(0xC0FF_EE06);
    let pieces = [
        ".", "..", "", "a", "b c", "\u{e9}", "...", "x\0", "\u{202e}", "ok.txt", "\\", ":",
    ];
    for round in 0..ROUNDS {
        let parts: Vec<&str> = (0..1 + rng.below(5)).map(|_| rng.pick(&pieces)).collect();
        let path = parts.join("/");
        match split_path(&path) {
            Ok(names) => {
                assert_eq!(names.len(), parts.len(), "round {round}");
                for n in names {
                    assert!(
                        n.as_str() != "." && n.as_str() != ".." && !n.as_str().contains('/'),
                        "round {round}"
                    );
                }
            }
            Err(_) => assert!(
                parts.iter().any(|p| validate_name(p).is_err()),
                "round {round}: {path:?} rejected although every component is legal"
            ),
        }
    }
}
