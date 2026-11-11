//! The JSON of the sign-in routes. 01 fixes the ports, not the bodies; the names follow the OAuth words the
//! clients already know (`access_token`, `refresh_token`, `expires_in`) and the port's own fields.

use contracts::{AuthorizationRedirect, BeginSignIn, ClientKind, ProviderList, SessionTokens};
use sc_ids::{RawToken, TokenKind};
use serde_json::{Value, json};

use crate::json_body::{self, optional, required, text};
use crate::registry::SchemaRef;
use crate::registry::request::seconds;

fn token_of(kind: TokenKind) -> impl FnOnce(&Value) -> Option<RawToken> {
    move |value| Some(RawToken::from_presented(kind, value.as_str()?.to_owned()))
}

fn client_kind(value: &Value) -> Option<ClientKind> {
    match value.as_str()? {
        "web" => Some(ClientKind::Web),
        "android" => Some(ClientKind::Android),
        "ios" => Some(ClientKind::Ios),
        "desktop" => Some(ClientKind::Desktop),
        _ => None,
    }
}

/// `{provider, return_to, client_kind, invite_token?, setup_token?, code_challenge?}` with the request's `host`.
pub(super) fn login_request(body: &Value, host: String) -> Option<BeginSignIn> {
    let members = json_body::object(
        body,
        &[
            "provider",
            "return_to",
            "client_kind",
            "invite_token",
            "setup_token",
            "code_challenge",
        ],
    )?;
    Some(BeginSignIn {
        provider: required(members, "provider", text)?,
        return_to: required(members, "return_to", text)?,
        client_kind: required(members, "client_kind", client_kind)?,
        invite_token: optional(members, "invite_token", token_of(TokenKind::Invite))?,
        setup_token: optional(members, "setup_token", token_of(TokenKind::Setup))?,
        code_challenge: optional(members, "code_challenge", text)?,
        host,
    })
}

/// `{provider}` of an identity-link attempt.
pub(super) fn link_request(body: &Value) -> Option<String> {
    let [provider] = json_body::fields(body, ["provider"])?;
    text(provider)
}

/// `{code, code_verifier}` of the mobile code exchange.
pub(super) fn code_exchange(body: &Value) -> Option<(RawToken, String)> {
    let [code, verifier] = json_body::fields(body, ["code", "code_verifier"])?;
    Some((token_of(TokenKind::MobileLoginCode)(code)?, text(verifier)?))
}

/// `{refresh_token}`.
pub(super) fn refresh_request(body: &Value) -> Option<RawToken> {
    let [token] = json_body::fields(body, ["refresh_token"])?;
    token_of(TokenKind::Refresh)(token)
}

pub(super) fn providers(list: &ProviderList) -> Value {
    let providers: Vec<Value> = list
        .providers
        .iter()
        .map(|provider| json!({"key": provider.key, "display_name": provider.display_name}))
        .collect();
    json!({"providers": providers, "setup_required": list.setup_required})
}

pub(super) fn authorization(redirect: &AuthorizationRedirect) -> Value {
    json!({"authorization_url": redirect.url})
}

/// The tokens a native client keeps; the plaintext is in this answer only.
pub(super) fn tokens(tokens: &SessionTokens) -> Value {
    json!({
        "access_token": tokens.access.expose(),
        "refresh_token": tokens.refresh.expose(),
        "expires_in": seconds(tokens.expires_in),
    })
}

pub(super) const LOGIN_REQUEST: SchemaRef = SchemaRef {
    name: "LoginAttemptRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["provider", "return_to", "client_kind"],
            "additionalProperties": false,
            "properties": {
                "provider": {"type": "string"},
                "return_to": {"type": "string", "description": "A relative path."},
                "client_kind": {"type": "string", "enum": ["web", "android", "ios", "desktop"]},
                "invite_token": {"type": ["string", "null"]},
                "setup_token": {"type": ["string", "null"]},
                "code_challenge": {"type": ["string", "null"], "description": "Required for android and ios."}
            }
        })
    },
};

pub(super) const LINK_REQUEST: SchemaRef = SchemaRef {
    name: "IdentityLinkAttemptRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["provider"],
            "additionalProperties": false,
            "properties": {"provider": {"type": "string"}}
        })
    },
};

pub(super) const AUTHORIZATION: SchemaRef = SchemaRef {
    name: "AuthorizationRedirect",
    build: || {
        json!({
            "type": "object",
            "required": ["authorization_url"],
            "properties": {"authorization_url": {"type": "string"}}
        })
    },
};

pub(super) const PROVIDER_LIST: SchemaRef = SchemaRef {
    name: "ProviderList",
    build: || {
        json!({
            "type": "object",
            "required": ["providers", "setup_required"],
            "properties": {
                "providers": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["key", "display_name"],
                        "properties": {"key": {"type": "string"}, "display_name": {"type": "string"}}
                    }
                },
                "setup_required": {"type": "boolean"}
            }
        })
    },
};

pub(super) const CODE_EXCHANGE_REQUEST: SchemaRef = SchemaRef {
    name: "MobileCodeExchangeRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["code", "code_verifier"],
            "additionalProperties": false,
            "properties": {"code": {"type": "string"}, "code_verifier": {"type": "string"}}
        })
    },
};

pub(super) const REFRESH_REQUEST: SchemaRef = SchemaRef {
    name: "RefreshRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["refresh_token"],
            "additionalProperties": false,
            "properties": {"refresh_token": {"type": "string"}}
        })
    },
};

pub(super) const SESSION_TOKENS: SchemaRef = SchemaRef {
    name: "SessionTokens",
    build: || {
        json!({
            "type": "object",
            "required": ["access_token", "refresh_token", "expires_in"],
            "properties": {
                "access_token": {"type": "string"},
                "refresh_token": {"type": "string"},
                "expires_in": {"type": "integer", "description": "Seconds."}
            }
        })
    },
};
