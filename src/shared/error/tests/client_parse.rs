#![cfg(all(feature = "client", feature = "server"))]

use sc_error::{
    ApiError, BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode, ResyncReason,
    TransportKind,
};
use std::time::Duration;
use uuid::Uuid;

fn request_id() -> CorrelationId {
    CorrelationId::new(Uuid::from_u128(0x0192_8f3a_7b00_7abc_8def_0123_4567_89ab))
}

fn round_trip(code: ErrorCode, params: BusinessParams) -> ApiError {
    let error = DomainError::Business(BusinessError::new(code, params));
    let body = serde_json::to_vec(&error.problem(request_id())).expect("serialises");
    ApiError::parse(code.status(), &[], &body)
}

#[test]
fn business_error_round_trips_with_all_parameters() {
    let params = BusinessParams {
        need_blocks: vec![1, 5, 4_000_000_000],
        retry_after: None,
        reason: Some(ResyncReason::CursorAhead),
        min_version: Some("2.0.1".to_owned()),
    };

    let parsed = round_trip(ErrorCode::NeedBlocks, params.clone());

    assert_eq!(
        parsed,
        ApiError::Api {
            code: ErrorCode::NeedBlocks,
            params,
            correlation_id: Some(request_id()),
        }
    );
}

#[test]
fn rate_limited_and_overloaded_carry_the_same_retry_after() {
    for code in [ErrorCode::RateLimited, ErrorCode::Overloaded] {
        let params = BusinessParams {
            retry_after: Some(Duration::from_secs(17)),
            ..BusinessParams::default()
        };

        let parsed = round_trip(code, params);

        assert_eq!(parsed.retry_after(), Some(Duration::from_secs(17)));
        match parsed {
            ApiError::Api {
                code: parsed_code, ..
            } => assert_eq!(parsed_code, code),
            ApiError::Transport(kind) => panic!("unexpected transport {kind:?}"),
        }
    }
}

#[test]
fn every_code_survives_a_round_trip() {
    for code in ErrorCode::ALL {
        let parsed = round_trip(*code, BusinessParams::default());

        assert_eq!(
            parsed,
            ApiError::Api {
                code: *code,
                params: BusinessParams::default(),
                correlation_id: Some(request_id()),
            }
        );
    }
}

#[test]
fn unknown_code_becomes_internal_and_keeps_request_id() {
    let body = format!(
        r#"{{"code":"brand_new","status":418,"request_id":"{}"}}"#,
        request_id()
    );

    let parsed = ApiError::parse(418, &[], body.as_bytes());

    assert_eq!(
        parsed,
        ApiError::Api {
            code: ErrorCode::Internal,
            params: BusinessParams::default(),
            correlation_id: Some(request_id()),
        }
    );
}

#[test]
fn empty_and_non_json_bodies_become_internal_with_header_request_id() {
    let id = request_id().to_string();
    let headers = [("X-Request-Id", id.as_str())];

    for body in [
        &b""[..],
        b"<html>502</html>",
        b"null",
        b"[1,2]",
        b"\xff\xfe",
    ] {
        let parsed = ApiError::parse(502, &headers, body);

        assert_eq!(
            parsed,
            ApiError::Api {
                code: ErrorCode::Internal,
                params: BusinessParams::default(),
                correlation_id: Some(request_id()),
            }
        );
    }
}

#[test]
fn retry_after_header_is_used_when_the_body_has_none() {
    let body = br#"{"code":"service_unavailable"}"#;

    let parsed = ApiError::parse(503, &[("retry-after", " 30 ")], body);

    assert_eq!(parsed.retry_after(), Some(Duration::from_secs(30)));
}

#[test]
fn transport_failure_is_typed() {
    assert_eq!(
        ApiError::parse(0, &[], b""),
        ApiError::Transport(TransportKind::Other)
    );
}

#[test]
fn unknown_reason_and_malformed_parameters_are_dropped_not_fatal() {
    let body = br#"{"code":"resync_required","reason":"moon_phase","need_blocks":[1,"x"],"retry_after":-4,"min_version":7,"request_id":"not-a-uuid"}"#;

    let parsed = ApiError::parse(410, &[], body);

    assert_eq!(
        parsed,
        ApiError::Api {
            code: ErrorCode::ResyncRequired,
            params: BusinessParams::default(),
            correlation_id: None,
        }
    );
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn fuzzed_bodies_never_panic_and_unknown_codes_are_internal() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    let seeds: [&[u8]; 3] = [
        br#"{"code":"need_blocks","need_blocks":[1,2,3],"retry_after":9,"reason":"epoch_changed","request_id":"0192-x"}"#,
        br#"{"code":"rate_limited","retry_after":1e400}"#,
        br#"{"code":{"a":[1,2,{"b":null}]},"need_blocks":"no"}"#,
    ];
    for iteration in 0..4000 {
        let mut body: Vec<u8> = if iteration % 2 == 0 {
            let len = (rng.next() % 96) as usize;
            (0..len).map(|_| rng.next().to_le_bytes()[0]).collect()
        } else {
            seeds[iteration % seeds.len()].to_vec()
        };
        if iteration % 2 == 1 && !body.is_empty() {
            for _ in 0..3 {
                let at = (rng.next() as usize) % body.len();
                body[at] = rng.next().to_le_bytes()[0];
            }
        }
        let status = u16::try_from(rng.next() % 1000).expect("below 1000");

        let parsed = ApiError::parse(status, &[("retry-after", "x")], &body);

        if let ApiError::Api { code, .. } = parsed {
            let text = String::from_utf8_lossy(&body);
            if !ErrorCode::ALL
                .iter()
                .any(|known| text.contains(known.as_str()))
            {
                assert_eq!(code, ErrorCode::Internal, "body {text}");
            }
        }
    }
}
