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
        let body = token.expose().strip_prefix("sc_ses_").expect("prefix");
        let bytes = URL_SAFE_NO_PAD.decode(body).expect("url-safe base64");
        let bytes: [u8; 32] = bytes.try_into().expect("256 bits");
        assert!(seen.insert(bytes), "token collision");
    }
}

#[test]
fn token_text_is_url_safe_without_padding() {
    let token = new_token(TokenKind::Share).expect("rng");

    assert_eq!(token.kind(), TokenKind::Share);
    let body = token.expose().strip_prefix("sc_shr_").expect("prefix");
    assert_eq!(body.len(), 43);
    assert!(
        body.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
}

const KINDS: [(TokenKind, &str); 8] = [
    (TokenKind::Session, "sc_ses_"),
    (TokenKind::Refresh, "sc_ref_"),
    (TokenKind::DeviceToken, "sc_dev_"),
    (TokenKind::DeviceCode, "sc_dcd_"),
    (TokenKind::Invite, "sc_inv_"),
    (TokenKind::Setup, "sc_set_"),
    (TokenKind::Share, "sc_shr_"),
    (TokenKind::MobileLoginCode, "sc_lgc_"),
];

#[test]
fn new_token_starts_with_the_prefix_of_its_kind() {
    // Arrange
    let kinds = KINDS;

    // Act
    let tokens: Vec<(TokenKind, &str, String)> = kinds
        .iter()
        .map(|(kind, prefix)| {
            let token = new_token(*kind).expect("rng");
            (*kind, *prefix, token.expose().to_owned())
        })
        .collect();

    // Assert
    for (kind, prefix, value) in &tokens {
        assert_eq!(kind.prefix(), *prefix);
        assert!(value.starts_with(prefix), "{kind:?}");
        assert_eq!(value.len(), prefix.len() + 43, "{kind:?}");
    }
    let distinct: HashSet<&str> = kinds.iter().map(|(_, prefix)| *prefix).collect();
    assert_eq!(distinct.len(), kinds.len());
}

#[test]
fn a_presented_token_is_of_its_kind_only_with_that_kind_prefix() {
    // Arrange
    let issued: Vec<(TokenKind, String)> = KINDS
        .iter()
        .map(|(kind, _)| (*kind, new_token(*kind).expect("rng").expose().to_owned()))
        .collect();

    // Act
    let mut answers = Vec::new();
    for (issued_kind, value) in &issued {
        for (expected, _) in KINDS {
            let presented = RawToken::from_presented(expected, value.clone());
            answers.push((*issued_kind == expected, presented.is_of_its_kind()));
        }
    }
    let bare = RawToken::from_presented(TokenKind::Session, "no-prefix".to_owned());

    // Assert
    assert!(answers.iter().all(|(same, of_kind)| same == of_kind));
    assert!(!bare.is_of_its_kind());
}

#[test]
fn the_hash_covers_the_prefix() {
    // Arrange
    let token = new_token(TokenKind::Session).expect("rng");
    let body = token.expose().strip_prefix("sc_ses_").expect("prefix");

    // Act
    let full = sha256_token(&token);
    let refresh_labelled = sha256_token(format!("sc_ref_{body}"));

    // Assert
    assert_eq!(full, sha256_token(token.expose()));
    assert_ne!(full, sha256_token(body));
    assert_ne!(full, refresh_labelled);
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

#[test]
fn random_text_is_43_url_safe_characters_so_it_carries_no_type_prefix() {
    // Arrange
    let draws = 1000;

    // Act
    let texts: Vec<String> = (0..draws)
        .map(|_| sc_ids::new_random_text().expect("random text"))
        .collect();

    // Assert
    let distinct: std::collections::HashSet<&String> = texts.iter().collect();
    assert_eq!(distinct.len(), draws);
    for text in &texts {
        assert_eq!(text.len(), 43);
        assert!(
            text.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
    }
}
