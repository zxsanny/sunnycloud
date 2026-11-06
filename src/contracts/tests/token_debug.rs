//! AC-5 (NFT-SEC-16): a DTO that carries a token never prints it.

use std::time::Duration;

use contracts::{
    AuthorizationRedirect, BeginSignIn, BrowserBinding, CallbackParams, ClientKind, Credential,
    DeviceAuthorization, DeviceToken, InviteToken, SessionTokens, SignInOutcome,
};
use sc_ids::{RawToken, TokenKind};

const ACCESS: &str = "access-secret-AAAA1111";
const REFRESH: &str = "refresh-secret-BBBB2222";
const DEVICE_TOKEN: &str = "device-token-secret-CCCC3333";
const DEVICE_CODE: &str = "device-code-secret-DDDD4444";
const USER_CODE_TEXT: &str = "BCDFGHJK";
const INVITE: &str = "invite-secret-EEEE5555";
const SETUP: &str = "setup-secret-FFFF6666";
const MOBILE_CODE: &str = "mobile-code-secret-GGGG7777";
const BINDING: &str = "binding-secret-HHHH8888";
const CALLBACK: &str = "callback-secret-IIII9999";

fn token(kind: TokenKind, value: &str) -> RawToken {
    RawToken::from_presented(kind, value.to_owned())
}

fn assert_hidden(printed: &str, secrets: &[&str]) {
    for secret in secrets {
        assert!(!printed.contains(secret), "{secret} printed in {printed}");
    }
    assert!(
        printed.contains("<redacted>"),
        "no placeholder in {printed}"
    );
}

fn user_code() -> sc_ids::UserCode {
    // Only the generator builds a `UserCode`; its value is whatever it drew.
    sc_ids::new_user_code().expect("user code")
}

#[test]
fn session_tokens_print_a_placeholder() {
    let tokens = SessionTokens {
        access: token(TokenKind::Session, ACCESS),
        refresh: token(TokenKind::Refresh, REFRESH),
        expires_in: Duration::from_secs(900),
    };

    assert_hidden(&format!("{tokens:?} {tokens:#?}"), &[ACCESS, REFRESH]);
}

#[test]
fn invite_and_device_tokens_print_a_placeholder() {
    let invite = InviteToken(token(TokenKind::Invite, INVITE));
    let setup = InviteToken(token(TokenKind::Setup, SETUP));
    let device = DeviceToken(token(TokenKind::DeviceToken, DEVICE_TOKEN));

    assert_hidden(&format!("{invite:?}"), &[INVITE]);
    assert_hidden(&format!("{setup:?}"), &[SETUP]);
    assert_hidden(&format!("{device:?}"), &[DEVICE_TOKEN]);
}

#[test]
fn device_authorization_hides_the_device_code_and_the_user_code() {
    let user_code = user_code();
    let authorization = DeviceAuthorization {
        device_code: token(TokenKind::DeviceCode, DEVICE_CODE),
        user_code: user_code.clone(),
        interval: Duration::from_secs(5),
        expires_in: Duration::from_secs(600),
    };

    assert_hidden(
        &format!("{authorization:?} {authorization:#?}"),
        &[DEVICE_CODE, user_code.as_str(), USER_CODE_TEXT],
    );
}

#[test]
fn credentials_and_sign_in_values_print_a_placeholder() {
    let session = Credential::Session(token(TokenKind::Session, ACCESS));
    let device = Credential::Device(token(TokenKind::DeviceToken, DEVICE_TOKEN));
    let outcome = SignInOutcome::MobileCode {
        code: token(TokenKind::MobileLoginCode, MOBILE_CODE),
        return_to: "/".to_owned(),
    };
    let redirect = AuthorizationRedirect {
        url: "https://idp.example/authorize".to_owned(),
        browser_binding: BrowserBinding(BINDING.to_owned()),
    };
    let params = CallbackParams(vec![("code".to_owned(), CALLBACK.to_owned())]);
    let begin = BeginSignIn {
        provider: "github".to_owned(),
        return_to: "/".to_owned(),
        client_kind: ClientKind::Web,
        invite_token: Some(token(TokenKind::Invite, INVITE)),
        setup_token: Some(token(TokenKind::Setup, SETUP)),
        code_challenge: None,
        host: "cloud.example".to_owned(),
    };

    assert_hidden(&format!("{session:?}"), &[ACCESS]);
    assert_hidden(&format!("{device:?}"), &[DEVICE_TOKEN]);
    assert_hidden(&format!("{outcome:?}"), &[MOBILE_CODE]);
    assert_hidden(&format!("{redirect:?}"), &[BINDING]);
    assert_hidden(&format!("{params:?}"), &[CALLBACK]);
    assert_hidden(&format!("{begin:?}"), &[INVITE, SETUP]);
}
