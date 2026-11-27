//! AC-1, parser part: every answer kind of the script file parses into the answer FD plays, an unknown kind or
//! field is refused, and the default answers every request after the queue.

use std::path::PathBuf;
use std::time::Duration;

use media_shared::decoder_protocol::{DecodeError, LimitKind, SelfCheckItem, SelfCheckReport};
use media_shared::policy::{MediaKind, Probe};
use serde_json::{Value, json};
use sunnycloud_fake_decoder::{
    Answer, IdleOutage, Output, Queue, Script, ScriptError, capabilities,
};

fn photo() -> Probe {
    Probe {
        kind: MediaKind::Photo,
        container: "jpeg".into(),
        moov_first: false,
        video: None,
        audio: None,
        duration_ms: None,
    }
}

fn photo_json() -> Value {
    json!({"kind": "photo", "container": "jpeg", "moov_first": false, "video": null, "audio": null,
           "duration_ms": null})
}

/// A script whose `render_still` queue holds `answers`.
fn render_queue(answers: Value) -> Result<Script, ScriptError> {
    Script::parse(&json!({"render_still": {"queue": answers, "default": "hang"}}).to_string())
}

fn parsed_queue(answers: Value) -> Vec<Answer> {
    let script = render_queue(answers).expect("the script parses");
    script.render_still.queue.into_iter().collect()
}

#[test]
fn parse_every_answer_kind_gives_the_answer_fd_plays() {
    // Arrange
    let answers = json!([
        {"probe": photo_json()},
        {"image": {"width": 320, "height": 240}},
        {"transcode": {"probe_of_output": photo_json()}},
        {"error": {"limit_exceeded": {"limit": "wall"}}},
        "hang",
        "silent",
        {"alive_only": {"every_ms": 50}},
        {"garbage": {"chunks": [{"hex": "ffff"}]}},
        {"over_time": {"delay_ms": 8000, "alive_every_ms": 500, "answer": {"error": "stalled"}}},
        {"over_time": {"delay_ms": 300, "answer": "silent"}},
        {"outage": {"refuse_ms": 3000}},
    ]);

    // Act
    let parsed = parsed_queue(answers);

    // Assert
    assert_eq!(
        parsed,
        [
            Answer::Probe(photo()),
            Answer::image(320, 240),
            Answer::transcode(photo()),
            Answer::Error(DecodeError::LimitExceeded {
                limit: LimitKind::Wall
            }),
            Answer::Hang,
            Answer::Silent,
            Answer::AliveOnly {
                every: Duration::from_millis(50)
            },
            Answer::Garbage(vec![0xff, 0xff]),
            Answer::OverTime {
                delay: Duration::from_secs(8),
                alive_every: Some(Duration::from_millis(500)),
                answer: Box::new(Answer::Error(DecodeError::Stalled)),
            },
            Answer::OverTime {
                delay: Duration::from_millis(300),
                alive_every: None,
                answer: Box::new(Answer::Silent),
            },
            Answer::Outage {
                refuse: Duration::from_secs(3)
            },
        ]
    );
}

#[test]
fn parse_every_output_kind_gives_the_file_fd_plants() {
    // Arrange
    let outputs = [
        (json!("file"), Output::File),
        (
            json!({"bytes": {"chunks": [{"text": "RIFF"}, {"hex": "1a000000"}]}}),
            Output::Bytes(b"RIFF\x1a\x00\x00\x00".to_vec()),
        ),
        (json!("absent"), Output::Absent),
        (
            json!({"symlink": {"target": "/run/secrets/worker_api_token"}}),
            Output::Symlink {
                target: PathBuf::from("/run/secrets/worker_api_token"),
            },
        ),
        (
            json!({"hard_link": {"in_file": "source"}}),
            Output::HardLink {
                in_file: "source".into(),
            },
        ),
        (json!("fifo"), Output::Fifo),
        (
            json!({"sparse": {"bytes": 1_073_741_824u64}}),
            Output::Sparse { bytes: 1 << 30 },
        ),
        (json!("directory"), Output::Directory),
        (
            json!({"swap": {"target": "/run/secrets/worker_api_token",
                            "bytes": {"chunks": [{"text": "RIFF"}]}, "times": 10_000}}),
            Output::Swap {
                target: PathBuf::from("/run/secrets/worker_api_token"),
                bytes: b"RIFF".to_vec(),
                times: 10_000,
            },
        ),
    ];
    let answers: Vec<Value> = outputs
        .iter()
        .map(|(output, _)| json!({"image": {"width": 320, "height": 240, "output": output}}))
        .collect();

    // Act
    let parsed = parsed_queue(Value::Array(answers));

    // Assert
    let expected: Vec<Answer> = outputs
        .into_iter()
        .map(|(_, output)| Answer::Image {
            width: 320,
            height: 240,
            output,
        })
        .collect();
    assert_eq!(parsed, expected);
}

#[test]
fn garbage_chunks_are_joined_in_order_and_framed_on_request() {
    // Arrange
    let answers = json!([
        {"garbage": {"chunks": [{"hex": "7bfffe7d"}, {"text": "ab"}, {"repeat": {"byte": 32, "count": 3}}]}},
        {"garbage": {"framed": true, "chunks": [{"text": "{}"}]}},
        {"garbage": {"framed": true, "chunks": [{"repeat": {"byte": 32, "count": 65537}}]}},
    ]);

    // Act
    let parsed = parsed_queue(answers);

    // Assert
    let mut oversize = 65_537u32.to_be_bytes().to_vec();
    oversize.extend(std::iter::repeat_n(b' ', 65_537));
    assert_eq!(
        parsed,
        [
            Answer::Garbage(vec![0x7b, 0xff, 0xfe, 0x7d, b'a', b'b', b' ', b' ', b' ']),
            Answer::Garbage(vec![0, 0, 0, 2, b'{', b'}']),
            Answer::Garbage(oversize),
        ]
    );
}

#[test]
fn an_unknown_answer_kind_is_a_parse_error() {
    // Arrange
    let answers = json!(["explode"]);

    // Act
    let result = render_queue(answers);

    // Assert
    assert!(matches!(result, Err(ScriptError::Json(_))), "{result:?}");
}

#[test]
fn an_unknown_field_anywhere_is_a_parse_error() {
    let scripts = [
        json!({"surprise": 1}),
        json!({"render_still": {"queue": [], "default": "hang", "extra": 1}}),
        json!({"render_still": {"default": {"image": {"width": 1, "height": 1, "colour": "red"}}}}),
        json!({"hello": {"build_timestamp": 1, "flavour": "x"}}),
    ];
    for script in scripts {
        // Act
        let result = Script::parse(&script.to_string());

        // Assert
        assert!(
            matches!(result, Err(ScriptError::Json(_))),
            "{script}: {result:?}"
        );
    }
}

#[test]
fn a_section_without_a_default_is_a_parse_error() {
    // Arrange
    let script = json!({"probe": {"queue": ["hang"]}});

    // Act
    let result = Script::parse(&script.to_string());

    // Assert
    assert!(matches!(result, Err(ScriptError::Json(_))), "{result:?}");
}

#[test]
fn bad_garbage_is_a_parse_error() {
    let cases = [
        json!({"garbage": {"chunks": [{"hex": "abc"}]}}),
        json!({"garbage": {"chunks": [{"hex": "zz"}]}}),
        json!({"garbage": {"chunks": [{"hex": "+f"}]}}),
        json!({"garbage": {"chunks": [{"repeat": {"byte": 0, "count": 20_000_000}}]}}),
    ];
    for answer in cases {
        // Act
        let result = render_queue(json!([answer]));

        // Assert
        assert!(
            matches!(result, Err(ScriptError::Json(_))),
            "{answer}: {result:?}"
        );
    }
}

#[test]
fn an_empty_script_is_the_documented_default() {
    // Act
    let script = Script::parse("{}").expect("an empty script parses");

    // Assert
    assert_eq!(script, Script::default());
    assert_eq!(
        script.probe,
        Queue::new(Answer::Error(DecodeError::Unsupported))
    );
    assert_eq!(
        script.render_still,
        Queue::new(Answer::Error(DecodeError::Corrupt))
    );
    assert_eq!(
        script.transcode,
        Queue::new(Answer::Error(DecodeError::Corrupt))
    );
}

#[test]
fn hello_self_check_and_idle_outage_take_the_given_values() {
    // Arrange
    let text = json!({
        "hello": {"build_timestamp": 1_690_000_000u64, "nvenc_probe_ok": true},
        "self_check": [{"name": "no_network", "ok": false}, {"name": "no_secrets", "ok": true}],
        "idle_outage": {"after_ms": 500, "refuse_ms": 3000},
    })
    .to_string();

    // Act
    let script = Script::parse(&text).expect("the script parses");

    // Assert
    let mut hello = capabilities();
    hello.build_timestamp = 1_690_000_000;
    hello.nvenc_probe_ok = true;
    assert_eq!(script.hello, hello);
    assert_eq!(
        script.self_check,
        SelfCheckReport(vec![
            SelfCheckItem {
                name: "no_network".into(),
                ok: false
            },
            SelfCheckItem {
                name: "no_secrets".into(),
                ok: true
            },
        ])
    );
    assert_eq!(
        script.idle_outage,
        Some(IdleOutage {
            after: Duration::from_millis(500),
            refuse: Duration::from_secs(3),
        })
    );
}

#[test]
fn the_default_answers_every_request_after_the_queue() {
    // Arrange
    let text = json!({"probe": {"queue": ["hang", "silent"], "default": {"error": "corrupt"}}});
    let mut script = Script::parse(&text.to_string()).expect("the script parses");

    // Act
    let played: Vec<Answer> = (0..4).map(|_| script.probe.next_answer()).collect();

    // Assert
    let corrupt = Answer::Error(DecodeError::Corrupt);
    assert_eq!(
        played,
        [Answer::Hang, Answer::Silent, corrupt.clone(), corrupt]
    );
}

#[test]
fn an_answer_the_protocol_would_refuse_is_a_script_error() {
    let cases = [
        json!({"image": {"width": 0, "height": 240}}),
        json!({"over_time": {"delay_ms": 1, "answer": {"image": {"width": 20000, "height": 1}}}}),
        json!({"error": {"encoder_failed": {"exit_code": 0}}}),
        json!({"image": {"width": 1, "height": 1, "output": {"sparse": {"bytes": 1u64 << 51}}}}),
    ];
    for answer in cases {
        // Act
        let result = render_queue(json!([answer]));

        // Assert
        assert!(
            matches!(result, Err(ScriptError::Unsendable(_))),
            "{answer}: {result:?}"
        );
    }
}

#[test]
fn a_hello_the_protocol_would_refuse_is_a_script_error() {
    // Arrange
    let text = json!({"hello": {"vips": "has spaces"}}).to_string();

    // Act
    let result = Script::parse(&text);

    // Assert
    assert!(
        matches!(result, Err(ScriptError::Unsendable(_))),
        "{result:?}"
    );
}

#[test]
fn a_zero_alive_interval_is_a_script_error() {
    let cases = [
        json!({"alive_only": {"every_ms": 0}}),
        json!({"over_time": {"delay_ms": 10, "alive_every_ms": 0, "answer": "hang"}}),
    ];
    for answer in cases {
        // Act
        let result = render_queue(json!([answer]));

        // Assert
        assert!(
            matches!(result, Err(ScriptError::ZeroInterval(_))),
            "{answer}: {result:?}"
        );
    }
}

#[test]
fn a_hard_link_to_a_path_instead_of_a_name_in_in_is_a_script_error() {
    for name in ["../../etc/passwd", "", ".."] {
        // Arrange
        let answer =
            json!({"image": {"width": 1, "height": 1, "output": {"hard_link": {"in_file": name}}}});

        // Act
        let result = render_queue(json!([answer]));

        // Assert
        assert!(
            matches!(result, Err(ScriptError::NotAFileName(_))),
            "{name:?}: {result:?}"
        );
    }
}

#[test]
fn a_swap_of_zero_or_too_many_exchanges_is_a_script_error() {
    for times in [0u32, 1_000_001] {
        // Arrange
        let answer = json!({"image": {"width": 1, "height": 1, "output": {"swap": {
            "target": "/run/secrets/worker_api_token", "bytes": {"chunks": [{"text": "RIFF"}]}, "times": times}}}});

        // Act
        let result = render_queue(json!([answer]));

        // Assert
        assert!(
            matches!(result, Err(ScriptError::SwapTimes(t)) if t == times),
            "{times}: {result:?}"
        );
    }
}

#[test]
fn a_missing_script_file_is_a_read_error() {
    // Arrange
    let dir = tempfile::tempdir().expect("temporary folder");

    // Act
    let result = Script::read(&dir.path().join("absent.json"));

    // Assert
    assert!(
        matches!(result, Err(ScriptError::Read { .. })),
        "{result:?}"
    );
}
