mod common;

use common::{folder, key_of};
use serde_json::Value;

use sc_names::{
    FsProfile, NameError, NameLookup, Portable, Reason, escape_name, portable_check, split_path,
};

const CORPUS: &str = include_str!("../names_corpus.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).expect("names_corpus.json is valid JSON")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|s| s.as_str().expect("string").to_owned())
        .collect()
}

fn error_named(name: &str) -> NameError {
    match name {
        "Empty" => NameError::Empty,
        "Dot" => NameError::Dot,
        "DotDot" => NameError::DotDot,
        "ContainsSlash" => NameError::ContainsSlash,
        "ContainsNul" => NameError::ContainsNul,
        "ControlChar" => NameError::ControlChar,
        "BidiControl" => NameError::BidiControl,
        other => panic!("corpus names an unknown error {other}"),
    }
}

fn reason_named(name: &str) -> Reason {
    match name {
        "ReservedDeviceName" => Reason::ReservedDeviceName,
        "TrailingDotOrSpace" => Reason::TrailingDotOrSpace,
        "ForbiddenChar" => Reason::ForbiddenChar,
        "AlternateDataStream" => Reason::AlternateDataStream,
        "OverPlatformLimit" => Reason::OverPlatformLimit,
        "LoneSurrogate" => Reason::LoneSurrogate,
        "CaseCollision" => Reason::CaseCollision,
        "NfcNfdCollision" => Reason::NfcNfdCollision,
        other => panic!("corpus names an unknown reason {other}"),
    }
}

fn profile_named(name: &str) -> FsProfile {
    match name {
        "windows_ntfs" => FsProfile::WindowsNtfs,
        "posix_case_sensitive" => FsProfile::PosixCaseSensitive,
        "case_insensitive_preserving" => FsProfile::CaseInsensitivePreserving,
        other => panic!("corpus names an unknown profile {other}"),
    }
}

fn is_collision(r: &Reason) -> bool {
    matches!(r, Reason::CaseCollision | Reason::NfcNfdCollision)
}

#[test]
fn corpus_has_forty_cases_for_each_profile() {
    let corpus = corpus();
    let profiles = corpus["profiles"].as_object().expect("profiles object");
    assert_eq!(profiles.len(), 3);
    for (name, profile) in profiles {
        let cases = profile["cases"].as_array().expect("cases");
        assert_eq!(cases.len(), 40, "{name}");
        let mut ids: Vec<&str> = cases
            .iter()
            .map(|c| c["id"].as_str().expect("id"))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 40, "{name}: ids are unique");
    }
}

#[test]
fn corpus_over_split_path_portable_check_and_escape_name() {
    let corpus = corpus();
    let mut checked = 0;
    for (profile_name, profile_data) in corpus["profiles"].as_object().expect("profiles") {
        let profile = profile_named(profile_name);
        let existing: Vec<String> = strings(&profile_data["folder"]);
        let state = folder(&existing.iter().map(String::as_str).collect::<Vec<_>>());
        for case in profile_data["cases"].as_array().expect("cases") {
            let id = format!("{profile_name}/{}", case["id"].as_str().expect("id"));
            let path = case["path"].as_str().expect("path");
            let split = split_path(path);
            if let Some(expected) = case.get("error") {
                assert_eq!(
                    split.unwrap_err(),
                    error_named(expected.as_str().expect("error name")),
                    "{id}"
                );
                checked += 1;
                continue;
            }
            let components = split.unwrap_or_else(|e| panic!("{id}: split_path failed with {e}"));
            let expected_components = strings(&case["components"]);
            let got: Vec<&str> = components.iter().map(|c| c.as_str()).collect();
            assert_eq!(got, expected_components, "{id}");
            for component in &components {
                let s = component.as_str();
                assert!(
                    s != "." && s != ".." && !s.contains('/'),
                    "{id}: traversal component {s:?}"
                );
                let escaped = escape_name(component, profile, &state);
                assert!(
                    !state.is_taken(&key_of(escaped.as_str())),
                    "{id}: {:?} collides",
                    escaped.as_str()
                );
            }
            let leaf = components.last().expect("a split path has components");
            let expected_reasons: Vec<Reason> = strings(&case["leaf_reasons"])
                .iter()
                .map(|r| reason_named(r))
                .collect();
            let escaped = escape_name(leaf, profile, &state);
            assert_eq!(escaped.reasons(), expected_reasons, "{id}");
            assert_eq!(
                escaped.as_str(),
                case["leaf_escaped"].as_str().expect("leaf_escaped"),
                "{id}"
            );
            if expected_reasons.is_empty() {
                assert_eq!(escaped.as_str(), leaf.as_str(), "{id}: unchanged");
            }
            let intrinsic: Vec<Reason> = expected_reasons
                .iter()
                .copied()
                .filter(|r| !is_collision(r))
                .collect();
            let expected_portable = if intrinsic.is_empty() {
                Portable::Ok
            } else {
                Portable::NeedsEscape(intrinsic)
            };
            assert_eq!(portable_check(leaf, profile), expected_portable, "{id}");
            assert_eq!(
                escape_name(leaf, profile, &state),
                escaped,
                "{id}: deterministic"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 120);
}

#[test]
fn escaped_corpus_names_are_always_free_in_their_folder() {
    let corpus = corpus();
    for (profile_name, profile_data) in corpus["profiles"].as_object().expect("profiles") {
        let profile = profile_named(profile_name);
        let existing = strings(&profile_data["folder"]);
        let mut state = folder(&existing.iter().map(String::as_str).collect::<Vec<_>>());
        for case in profile_data["cases"].as_array().expect("cases") {
            let Ok(components) = split_path(case["path"].as_str().expect("path")) else {
                continue;
            };
            let Some(leaf) = components.last() else {
                continue;
            };
            let escaped = escape_name(leaf, profile, &state);
            assert!(
                state.insert(key_of(escaped.as_str())),
                "{profile_name}: {:?} overwrote a sibling",
                escaped.as_str()
            );
        }
    }
}
