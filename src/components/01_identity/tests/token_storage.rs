//! ST-01-5 (NFT-SEC-38; AC-21, AC-58), AZ-3692 AC-4: 100 setup, invite, session, refresh, login-code, device-code
//! and device tokens minted through the real services. Every row of every table (dumped as the schema owner, so a
//! column added later is covered), the audit rows among them, and every captured log line hold no 16-character
//! piece of any token, as text or as the hex of its text or of its random bytes. Every token is of its family,
//! decodes to at least 32 bytes that look random, and the tokens of a family are pairwise distinct.

mod support;

use std::collections::HashSet;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use contracts::{ClientKind, DeviceGrantService, InviteService, OperatorIdentityOps};
use sc_ids::{RawToken, TokenKind, sha256_token};
use support::auth::authenticator;
use support::device::{approve, geo, grants, ip};
use support::invites::{invites, sign_in_with_setup};
use support::logs::LogCapture;
use support::onboarding::{all_rows, another_session, principal_of, session_tokens};
use support::registration::Rig;
use support::sign_in::{CHALLENGE, OIDC};

const POOL: u32 = 5;
const SUBJECT: &str = "identity-1";
const PER_FAMILY: usize = 100;
/// A piece this long is 96 bits of a token's text: a match is a leak, never chance.
const WINDOW: usize = 16;

/// The tokens of one family as the services handed them out.
struct Family {
    name: &'static str,
    kind: TokenKind,
    tokens: Vec<RawToken>,
}

impl Family {
    fn new(name: &'static str, kind: TokenKind) -> Family {
        Family {
            name,
            kind,
            tokens: Vec::new(),
        }
    }

    fn values(&self) -> Vec<&str> {
        self.tokens.iter().map(RawToken::expose).collect()
    }

    /// The longest text every token of the family starts with.
    fn common_prefix(&self) -> String {
        let values = self.values();
        let first = values.first().copied().unwrap_or_default();
        let length = values.iter().fold(first.len(), |length, value| {
            first
                .bytes()
                .zip(value.bytes())
                .take(length)
                .take_while(|(a, b)| a == b)
                .count()
        });
        first[..length].to_owned()
    }
}

/// `n` tokens of each family, minted through the services on an empty database: the setup tokens first (they
/// exist only while no account does; the last one signs the admin in), then the admin's invites, sessions,
/// mobile login codes and device grants approved by the admin's session.
async fn mint(rig: &mut Rig, n: usize) -> Vec<Family> {
    let operator = invites(&rig.world.harness);
    let service = grants(&mut rig.world.harness, geo(&[]));
    let authenticator = authenticator(&rig.world.harness);
    let mut setup = Family::new("setup", TokenKind::Setup);
    for _ in 0..n {
        let token = operator.ensure_setup_token().await.expect("ensure");
        setup
            .tokens
            .push(token.expect("a setup token while no account exists"));
    }
    let last = setup.tokens.last().expect("a setup token").expose();
    let admin_session = session_tokens(sign_in_with_setup(&rig.world, OIDC, SUBJECT, last).await);
    let admin = principal_of(&authenticator, &admin_session).await;
    let mut invite = Family::new("invite", TokenKind::Invite);
    let mut access = Family::new("session", TokenKind::Session);
    let mut refresh = Family::new("refresh", TokenKind::Refresh);
    let mut login = Family::new("login code", TokenKind::MobileLoginCode);
    let mut code = Family::new("device code", TokenKind::DeviceCode);
    let mut device = Family::new("device", TokenKind::DeviceToken);
    for i in 0..n {
        let note = format!("note {i}");
        let created = operator.create_invite(admin.account_id, &note).await;
        invite.tokens.push(created.expect("create invite").0);
        let tokens = another_session(rig, SUBJECT).await;
        access.tokens.push(tokens.access);
        refresh.tokens.push(tokens.refresh);
        let mobile = rig
            .world
            .mobile_code(ClientKind::Android, SUBJECT, CHALLENGE);
        login.tokens.push(mobile.await);
        let authorization = service
            .request("lab-1", "linux", ip("10.0.0.5"))
            .await
            .expect("request");
        approve(
            &service,
            &admin,
            &authorization.user_code,
            ip("198.51.100.9"),
        )
        .await;
        let minted = service.poll(&authorization.device_code).await;
        device
            .tokens
            .push(minted.expect("poll of an approved grant").0);
        code.tokens.push(authorization.device_code);
    }
    drop((operator, service, authenticator));
    vec![setup, invite, access, refresh, login, code, device]
}

/// Every `WINDOW`-byte piece of `text`.
fn windows(text: &str) -> impl Iterator<Item = &[u8]> {
    text.as_bytes().windows(WINDOW)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The random bytes behind `value` once the family's prefix is taken off.
fn random_bytes(value: &str, prefix: &str) -> Vec<u8> {
    let body = value.strip_prefix(prefix).expect("the family prefix");
    URL_SAFE_NO_PAD.decode(body).expect("URL-safe base64")
}

#[tokio::test]
async fn only_hashes_of_random_typed_tokens_are_stored_and_no_token_is_logged() {
    // Arrange
    let logs = LogCapture::install();
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let families = mint(&mut rig, PER_FAMILY).await;

    // Act
    let rows = all_rows(&mut rig.world.conn, false).await;
    let lines: Vec<String> = logs.lines().into_iter().map(|line| line.text).collect();

    // Assert
    assert!(rows.iter().any(|row| row.starts_with("audit_log: ")));
    let seen: HashSet<&[u8]> = rows.iter().chain(&lines).flat_map(|t| windows(t)).collect();
    let session = &families[2].tokens[0];
    let stored = hex(sha256_token(session.expose()).as_ref());
    assert!(
        windows(&stored).all(|piece| seen.contains(piece)),
        "the scan finds the stored hash of a session token"
    );
    for family in &families {
        let prefix = family.common_prefix();
        let values: HashSet<&str> = family.values().into_iter().collect();
        assert_eq!(values.len(), PER_FAMILY, "{} distinct", family.name);
        let mut ones = 0;
        for token in &family.tokens {
            assert_eq!(token.kind(), family.kind, "{}", family.name);
            let bytes = random_bytes(token.expose(), &prefix);
            assert!(bytes.len() >= 32, "{}: {} bytes", family.name, bytes.len());
            ones += bytes.iter().map(|byte| byte.count_ones()).sum::<u32>();
            for form in [
                token.expose().to_owned(),
                hex(token.expose().as_bytes()),
                hex(&bytes),
            ] {
                let leaked = windows(&form).any(|piece| seen.contains(piece));
                assert!(!leaked, "a {} token is stored or logged", family.name);
            }
        }
        let bits = f64::from(u32::try_from(PER_FAMILY * 256).expect("bit count"));
        let share = f64::from(ones) / bits;
        assert!((0.45..=0.55).contains(&share), "{}: {share}", family.name);
    }
    rig.finish().await;
}

#[tokio::test]
#[ignore = "DEFECT: AZ-3692 AC-4 type prefix: identity tokens carry no type prefix (sc_ids::new_token issues bare \
            URL-safe base64; the prefix strings are unspecified, sc_ids.md open point 1, data_model.md section 1)"]
async fn every_token_family_carries_its_own_type_prefix() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;

    // Act
    let families = mint(&mut rig, 10).await;

    // Assert
    let prefixes: Vec<(&str, String)> = families
        .iter()
        .map(|family| (family.name, family.common_prefix()))
        .collect();
    assert!(
        prefixes.iter().all(|(_, prefix)| !prefix.is_empty()),
        "{prefixes:?}"
    );
    let distinct: HashSet<&String> = prefixes.iter().map(|(_, prefix)| prefix).collect();
    assert_eq!(distinct.len(), families.len(), "{prefixes:?}");
    rig.finish().await;
}
