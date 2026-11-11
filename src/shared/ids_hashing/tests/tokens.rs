use std::collections::HashSet;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sc_ids::{RawToken, TokenKind, UserCode, ct_eq, new_token, new_user_code, sha256_token};

const BASE20: &str = "BCDFGHJKLMNPQRSTVWXZ";

#[test]
fn a_million_tokens_carry_256_bits_and_never_collide() {
    let mut seen = HashSet::with_capacity(1_000_000);

    for _ in 0..1_000_000 {
        let token = new_token(TokenKind::Session).expect("rng");
        let bytes = URL_SAFE_NO_PAD
            .decode(token.expose())
            .expect("url-safe base64");
        let bytes: [u8; 32] = bytes.try_into().expect("256 bits");
        assert!(seen.insert(bytes), "token collision");
    }
}

#[test]
fn token_text_is_url_safe_without_padding() {
    let token = new_token(TokenKind::Share).expect("rng");

    assert_eq!(token.kind(), TokenKind::Share);
    assert_eq!(token.expose().len(), 43);
    assert!(
        token
            .expose()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
}

#[test]
fn debug_and_display_never_print_the_value() {
    let value = "s3cr3t-token-value_ABCDEFGHIJKLMNOPQRSTUV".to_string();
    let token = RawToken::from_presented(TokenKind::Invite, value.clone());

    let debug = format!("{token:?}");
    let display = format!("{token}");
    let pretty = format!("{token:#?}");

    for output in [&debug, &display, &pretty] {
        assert!(output.contains("<redacted>"), "{output}");
        assert!(!output.contains("s3cr3t"), "{output}");
        assert!(!output.contains("ABCDEFGHIJ"), "{output}");
    }
    let generated = new_token(TokenKind::Session).expect("rng");
    let printed = format!("{generated:?} {generated}");
    assert!(!printed.contains(generated.expose()));
    assert!(!printed.contains(&generated.expose()[..8]));
}

#[test]
fn sha256_token_matches_the_known_digest_for_str_and_raw_token() {
    // SHA-256("abc")
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    let from_str = sha256_token("abc");
    let from_raw = sha256_token(RawToken::from_presented(
        TokenKind::Refresh,
        "abc".to_string(),
    ));

    assert_eq!(
        from_str
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        expected
    );
    assert_eq!(from_str, from_raw);
    assert_ne!(from_str, sha256_token("abd"));
}

#[test]
fn ct_eq_compares_equal_unequal_and_different_lengths() {
    assert!(ct_eq(b"same bytes", b"same bytes"));
    assert!(!ct_eq(b"same bytes", b"same bytez"));
    assert!(!ct_eq(b"short", b"shorter"));
    assert!(ct_eq(b"", b""));
}

#[test]
fn user_code_is_eight_characters_from_the_base20_alphabet() {
    let mut distinct = HashSet::new();

    for _ in 0..2000 {
        let code = new_user_code().expect("rng");
        assert_eq!(code.as_str().len(), 8);
        assert!(code.as_str().chars().all(|c| BASE20.contains(c)), "{code}");
        distinct.insert(code.to_string());
    }

    assert!(
        distinct.len() > 1990,
        "codes must be random, got {} distinct",
        distinct.len()
    );
}

#[test]
fn user_code_uses_the_whole_alphabet() {
    let mut used = HashSet::new();

    for _ in 0..2000 {
        used.extend(new_user_code().expect("rng").as_str().chars());
    }

    assert_eq!(used.len(), BASE20.len());
}

#[test]
fn a_presented_user_code_ignores_case_hyphens_and_spaces() {
    // Arrange
    let generated = new_user_code().expect("rng");
    let typed = format!(
        "{}-{} ",
        generated.as_str()[..4].to_lowercase(),
        &generated.as_str()[4..]
    );

    // Act
    let parsed = UserCode::from_presented(&typed);

    // Assert
    assert_eq!(parsed, Some(generated));
}

#[test]
fn a_presented_user_code_that_cannot_exist_is_refused() {
    for text in [
        "",
        "BCDFGHJ",
        "BCDFGHJKL",
        "BCDFGHJA",
        "BCDFGHJ1",
        "BCDF-GHJ-é",
    ] {
        assert_eq!(UserCode::from_presented(text), None, "{text}");
    }
}
