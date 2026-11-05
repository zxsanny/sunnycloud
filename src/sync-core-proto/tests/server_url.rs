//! AC-3: plain HTTP refused (AC-54); open point 3 rules.

use sync_core_proto::{ServerUrl, UrlError};

#[test]
fn plain_http_is_refused_unless_the_caller_allows_it() {
    assert_eq!(
        ServerUrl::parse("http://sunnycloud.test", false),
        Err(UrlError::InsecureScheme)
    );
    let url = ServerUrl::parse("http://sunnycloud.test", true).expect("allowed");
    assert!(!url.is_https());
}

#[test]
fn https_is_accepted_and_normalised() {
    let url = ServerUrl::parse("HTTPS://SunnyCloud.Test:8443/prefix//", false).expect("https");

    assert!(url.is_https());
    assert_eq!(url.as_str(), "https://sunnycloud.test:8443/prefix");
    assert_eq!(
        ServerUrl::parse("https://sunnycloud.test/", false)
            .expect("https")
            .as_str(),
        "https://sunnycloud.test"
    );
    assert_eq!(
        ServerUrl::parse("https://[::1]:8080", false)
            .expect("ipv6")
            .as_str(),
        "https://[::1]:8080"
    );
}

#[test]
fn text_without_a_host_or_with_another_scheme_is_invalid() {
    for text in [
        "",
        "sunnycloud.test",
        "https://",
        "https:///path",
        "https://:443",
        "ftp://sunnycloud.test",
        "ws://sunnycloud.test",
        "//sunnycloud.test",
        "https:sunnycloud.test",
        "file:///etc/passwd",
    ] {
        assert_eq!(
            ServerUrl::parse(text, true),
            Err(UrlError::Invalid),
            "{text:?}"
        );
    }
}

#[test]
fn user_info_query_fragment_bad_port_and_whitespace_are_invalid() {
    for text in [
        "https://user@sunnycloud.test",
        "https://user:pw@sunnycloud.test",
        "https://sunnycloud.test?x=1",
        "https://sunnycloud.test/path?x=1",
        "https://sunnycloud.test#frag",
        "https://sunnycloud.test:0",
        "https://sunnycloud.test:65536",
        "https://sunnycloud.test:abc",
        "https://sunnycloud.test:",
        "https://sunny cloud.test",
        "https://sunnycloud.test/\n",
        "https://sun_nycloud.test",
        "https://[]",
        "https://[zz::1]",
    ] {
        assert_eq!(
            ServerUrl::parse(text, true),
            Err(UrlError::Invalid),
            "{text:?}"
        );
    }
}

#[test]
fn an_invalid_http_url_is_invalid_not_insecure() {
    assert_eq!(ServerUrl::parse("http://", false), Err(UrlError::Invalid));
    assert_eq!(
        ServerUrl::parse("http://user@h.test", false),
        Err(UrlError::Invalid)
    );
}
