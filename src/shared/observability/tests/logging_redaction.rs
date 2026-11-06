//! AC-1 (NFT-SEC-38): planted secrets and personal data never reach the output.

mod common;

use common::{capture, config};
use sc_observability::{LogFormat, log_debug, log_error, log_info, log_warn, request_span};
use tracing::Level;

const PLANTED: &[(&str, &str)] = &[
    ("access_token", "at_7f3a9c1e2b4d6f80a1b2c3d4e5f60718"),
    ("refresh_token", "rt_Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MA"),
    ("setup_token", "setup-5d41402abc4b2a76b9719d911017c592"),
    ("invite_token", "inv_8e2f1a7c9b3d4e5f6a7b8c9d0e1f2a3b"),
    ("share_token", "shr_q9w8e7r6t5y4u3i2o1p0aSdFgHjKlZxC"),
    ("code", "4/0AfJohXkOAuthCodeValue-123"),
    ("state", "st_c0ffee0123456789abcdef0123456789"),
    (
        "master_key_hex",
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
    ),
    (
        "master_key_b64",
        "ABEiM0RVZneImaq7zN3u/wARIjNEVWZ3iJmqu8zd7v8=",
    ),
    ("provider_secret", "GOCSPX-providerSecretValue_42"),
    ("path", "/srv/blobs/alice/Tax Return 2025.pdf"),
    ("email", "alice.example@mail.test"),
];

fn assert_clean(output: &str) {
    for (name, value) in PLANTED {
        assert!(!output.contains(value), "{name} leaked: {output}");
    }
}

#[test]
fn ac1_planted_values_in_raw_fields_of_any_name_never_appear_at_any_level() {
    let (sink, stats, ()) = capture(config(LogFormat::Json, Level::TRACE), || {
        macro_rules! at_every_level {
            ($($level:ident),*) => {$(
                tracing::$level!(
                    access_token = PLANTED[0].1, refresh_token = PLANTED[1].1, setup_token = PLANTED[2].1,
                    invite_token = PLANTED[3].1, share_token = PLANTED[4].1, code = PLANTED[5].1,
                    state = PLANTED[6].1, master_key_hex = PLANTED[7].1, master_key_b64 = PLANTED[8].1,
                    provider_secret = PLANTED[9].1, path = PLANTED[10].1, email = PLANTED[11].1,
                    "constant message"
                );
            )*};
        }
        at_every_level!(error, warn, info, debug, trace);
    });

    let lines = sink.lines();
    assert_eq!(lines.len(), 5, "one line per level");
    assert_clean(&sink.text());
    assert!(lines.iter().all(|line| line.contains("constant message")));
    assert_eq!(
        stats.rejected_fields,
        5 * PLANTED.len() as u64,
        "each drop is counted"
    );
}

#[test]
fn ac1_planted_values_in_the_id_status_and_route_slots_are_rejected_by_shape() {
    let (sink, stats, ()) = capture(config(LogFormat::Json, Level::INFO), || {
        for (_, value) in PLANTED {
            tracing::info!(
                request_id = *value,
                account_id = *value,
                node_id = *value,
                route = *value,
                status = 70_000_u64,
                "shape check"
            );
        }
    });

    assert_clean(&sink.text());
    let leaked_route: Vec<_> = sink
        .lines()
        .into_iter()
        .filter(|l| l.contains("\"route\""))
        .collect();
    assert!(leaked_route.is_empty(), "{leaked_route:?}");
    assert!(stats.rejected_fields >= 4 * PLANTED.len() as u64);
}

#[test]
fn ac1_planted_values_in_a_span_do_not_reach_the_events_inside_it() {
    let (sink, _, ()) = capture(config(LogFormat::Json, Level::ERROR), || {
        let span = tracing::error_span!(
            "request",
            request_id = PLANTED[0].1,
            access_token = PLANTED[0].1,
            email = PLANTED[11].1
        );
        let _entered = span.enter();
        log_error!("inside the span");
    });

    assert_clean(&sink.text());
    assert!(sink.text().contains("\"request_id\":null"));
}

#[test]
fn ac1_typed_calls_log_ids_and_codes_and_nothing_else() {
    let error = std::io::Error::other("write failed");
    let (sink, _, ()) = capture(config(LogFormat::Json, Level::TRACE), || {
        let span = request_span(sc_error::CorrelationId::new(uuid::Uuid::nil()));
        let _entered = span.enter();
        log_error!("a", status = 500, err = &error);
        log_warn!("b", duration_ms = 3);
        log_info!("c", route = "/api/v1/nodes/{id}");
        log_debug!("d");
    });

    assert_clean(&sink.text());
    assert_eq!(sink.lines().len(), 4);
}
