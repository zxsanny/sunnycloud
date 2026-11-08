use axum::http::{HeaderMap, HeaderValue};

use super::*;

fn guard() -> CsrfGuard {
    CsrfGuard {
        public_origin: "https://sunnycloud.test".to_owned(),
        clock: Arc::new(sc_clock::SystemClock::new()),
    }
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            HeaderName::from_static(name),
            HeaderValue::from_str(value).expect("header value"),
        );
    }
    map
}

#[test]
fn only_the_two_same_origin_proofs_pass() {
    // Arrange
    let cases: [(&[(&'static str, &str)], bool); 14] = [
        (&[("sec-fetch-site", "same-origin")], true),
        (&[("origin", "https://sunnycloud.test")], true),
        (&[("origin", "https://sunnycloud.test:443")], true),
        (
            &[
                ("sec-fetch-site", "cross-site"),
                ("origin", "https://sunnycloud.test"),
            ],
            true,
        ),
        (&[], false),
        (&[("sec-fetch-site", "cross-site")], false),
        (&[("sec-fetch-site", "same-site")], false),
        (&[("sec-fetch-site", "none")], false),
        (&[("origin", "null")], false),
        (&[("origin", "https://evil.example")], false),
        (&[("origin", "https://app.sunnycloud.test")], false),
        (&[("origin", "http://sunnycloud.test")], false),
        (&[("origin", "https://sunnycloud.test:8443")], false),
        (&[("origin", "https://sunnycloud.test.evil.example")], false),
    ];

    // Act
    let outcomes: Vec<bool> = cases
        .iter()
        .map(|(pairs, _)| guard().proves_same_origin(&headers(pairs)))
        .collect();

    // Assert
    let expected: Vec<bool> = cases.iter().map(|(_, passes)| *passes).collect();
    assert_eq!(outcomes, expected);
}

#[test]
fn a_repeated_proof_header_proves_nothing() {
    // Arrange
    let twice_site = headers(&[
        ("sec-fetch-site", "same-origin"),
        ("sec-fetch-site", "same-origin"),
    ]);
    let twice_origin = headers(&[
        ("origin", "https://sunnycloud.test"),
        ("origin", "https://sunnycloud.test"),
    ]);

    // Act
    let outcomes = [
        guard().proves_same_origin(&twice_site),
        guard().proves_same_origin(&twice_origin),
    ];

    // Assert
    assert_eq!(outcomes, [false, false]);
}
