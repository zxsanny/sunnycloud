//! AC-3 unit: the script parser turns errors, delays and silence into scripted actions.

use ipc_schema::ControlError;
use serde_json::json;
use sunnycloud_settings_testkit::script::{Answer, Hostile, Script, ScriptError, TEN_MB};

#[test]
fn errors_delays_silence_and_hostile_frames_parse_into_actions() {
    let text = r#"{
        "answers": {
            "add_pair": [{"error": {"kind": "RulesInvalid", "line": 3}}],
            "update_pair": [{"error": {"kind": "Internal", "correlation_id": "c-77"}}],
            "get_settings": [{"delay": {"ms": 4000, "then": "default"}}],
            "status": ["silent", {"result": null}],
            "list_pairs": [{"hostile": {"oversize": {"bytes": 10485760}}}, {"hostile": "invalid_json"},
                           {"hostile": "wrong_id"}, {"hostile": "markup_error"}, {"hostile": {"raw": "x"}}]
        }
    }"#;

    let script = Script::parse(text).expect("script parses");

    assert_eq!(
        script.answers["add_pair"],
        vec![Answer::Error(ControlError::RulesInvalid { line: 3 })]
    );
    assert_eq!(
        script.answers["update_pair"],
        vec![Answer::Error(ControlError::Internal {
            correlation_id: "c-77".to_owned()
        })]
    );
    assert_eq!(
        script.answers["get_settings"],
        vec![Answer::Delay {
            ms: 4000,
            then: Box::new(Answer::Default)
        }]
    );
    assert_eq!(
        script.answers["status"],
        vec![Answer::Silent, Answer::Result(json!(null))]
    );
    assert_eq!(
        script.answers["list_pairs"],
        vec![
            Answer::Hostile(Hostile::Oversize { bytes: TEN_MB }),
            Answer::Hostile(Hostile::InvalidJson),
            Answer::Hostile(Hostile::WrongId),
            Answer::Hostile(Hostile::MarkupError),
            Answer::Hostile(Hostile::Raw("x".to_owned())),
        ]
    );
}

#[test]
fn a_misspelled_method_is_rejected_instead_of_never_firing() {
    let outcome = Script::parse(r#"{"answers": {"set_sever": ["silent"]}}"#);

    assert!(matches!(outcome, Err(ScriptError::UnknownMethod(name)) if name == "set_sever"));
}

#[test]
fn an_unknown_answer_or_field_does_not_parse() {
    assert!(matches!(
        Script::parse(r#"{"answers": {"status": ["sleep"]}}"#),
        Err(ScriptError::Parse(_))
    ));
    assert!(matches!(
        Script::parse(r#"{"anwsers": {}}"#),
        Err(ScriptError::Parse(_))
    ));
}

#[test]
fn a_script_file_is_read_from_disk() {
    let path = std::env::temp_dir().join(format!("sc-script-{}.json", std::process::id()));
    std::fs::write(&path, r#"{"answers": {"status": ["silent"]}}"#).expect("write script");

    let script = Script::from_file(&path).expect("file parses");
    std::fs::remove_file(&path).expect("remove script");

    assert_eq!(script.answers["status"], vec![Answer::Silent]);
    assert!(matches!(
        Script::from_file(&path),
        Err(ScriptError::Read { .. })
    ));
}
