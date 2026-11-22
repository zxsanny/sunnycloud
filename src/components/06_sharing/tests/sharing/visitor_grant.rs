//! IT-06-3 grant part and ST-06-4 (AZ-3699 AC-3, AC-4, AC-6): a grant from `verify_password` opens its own
//! password link in `authorize_content` until its expiry and nothing else; a link without password ignores grants;
//! nothing is stored, so a revoke ends the grant at once; mutated, moved, empty and oversized grants are refused,
//! and a near-valid grant takes as long to refuse as an invalid one. The clock is the harness's manual clock and
//! `share_grant_ttl` its 500 ms.

use std::time::{Duration, Instant};

use contracts::{
    AccessGrant, CreatedShare, PublicShareError, PublicShareService, ShareContent, ShareOpen,
    ShareService,
};
use sc_ids::RawToken;
use sqlx::Connection;
use sunnycloud_sharing::PgShareService;

use crate::content_support::{Owner, Version, content_of};
use crate::harness::Harness;
use crate::visitor_support::{PASSWORD, VISITOR};

const OTHER_PASSWORD: &str = "battery staple";
const MICRO: Duration = Duration::from_micros(1);

/// Two password links with different passwords and a plain link, all on one live file.
struct Links {
    owner: Owner,
    file: Version,
    shares: PgShareService,
    first: CreatedShare,
    second: CreatedShare,
    plain: CreatedShare,
}

impl Links {
    async fn new(harness: &Harness) -> Links {
        let mut owner = Owner::new(harness).await;
        let file = owner.new_file(harness, owner.root, "f.txt").await;
        let shares = harness.shares();
        let create = async |password: Option<&str>| {
            shares
                .create(owner.account, file.node, None, password)
                .await
                .expect("create link")
        };
        let first = create(Some(PASSWORD)).await;
        let second = create(Some(OTHER_PASSWORD)).await;
        let plain = create(None).await;
        Links {
            owner,
            file,
            shares,
            first,
            second,
            plain,
        }
    }

    async fn authorize(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
    ) -> Result<ShareContent, PublicShareError> {
        self.shares
            .authorize_content(token, grant, VISITOR, false)
            .await
    }

    async fn open(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
    ) -> Result<ShareOpen, PublicShareError> {
        self.shares.open(token, grant, VISITOR).await
    }

    async fn grants(&self) -> (AccessGrant, AccessGrant) {
        let first = self
            .shares
            .verify_password(&self.first.token, PASSWORD, VISITOR)
            .await;
        let second = self
            .shares
            .verify_password(&self.second.token, OTHER_PASSWORD, VISITOR)
            .await;
        (first.expect("first grant"), second.expect("second grant"))
    }

    async fn finish(self, harness: Harness) {
        drop(self.shares);
        self.owner.finish().await;
        harness.finish().await;
    }
}

fn with_value(grant: &AccessGrant, value: String) -> AccessGrant {
    AccessGrant {
        value,
        expires_at: grant.expires_at,
    }
}

fn shifted(grant: &AccessGrant, micros: i64) -> AccessGrant {
    let by = Duration::from_micros(micros.unsigned_abs());
    let expires_at = if micros >= 0 {
        grant.expires_at.checked_add(by)
    } else {
        grant.expires_at.checked_sub(by)
    };
    AccessGrant {
        value: grant.value.clone(),
        expires_at: expires_at.expect("in range"),
    }
}

/// At least 1,000 grants derived from `own` (valid for the target link) that must all be refused there: a bit
/// flipped at every bit of every byte, every byte replaced by another URL-safe character, the expiry moved by
/// ±1..=330 µs (still in the future), the grant of `other` link, empty and oversized values.
fn mutants(own: &AccessGrant, other: &AccessGrant) -> Vec<AccessGrant> {
    let bytes = own.value.as_bytes();
    let mut out = Vec::new();
    for offset in 0..bytes.len() {
        for bit in 0..8 {
            let mut changed = bytes.to_vec();
            changed[offset] ^= 1 << bit;
            out.push(with_value(
                own,
                String::from_utf8_lossy(&changed).into_owned(),
            ));
        }
        let mut replaced = bytes.to_vec();
        replaced[offset] = if bytes[offset] == b'A' { b'_' } else { b'A' };
        out.push(with_value(
            own,
            String::from_utf8_lossy(&replaced).into_owned(),
        ));
    }
    for micros in 1..=330 {
        out.push(shifted(own, micros));
        out.push(shifted(own, -micros));
    }
    out.push(other.clone());
    out.push(with_value(own, String::new()));
    for extra in [1, 2, 64, 1024, 64 * 1024] {
        out.push(with_value(
            own,
            format!("{}{}", own.value, "A".repeat(extra)),
        ));
    }
    out
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

#[tokio::test]
async fn a_grant_opens_only_its_own_link_while_valid_and_plain_links_ignore_grants() {
    // Arrange
    let harness = Harness::new().await;
    let links = Links::new(&harness).await;
    let (grant, _) = links.grants().await;
    let mut tampered = grant.value.clone().into_bytes();
    tampered[10] = if tampered[10] == b'A' { b'B' } else { b'A' };
    let tampered = with_value(&grant, String::from_utf8(tampered).expect("ascii"));

    // Act
    let own = links.authorize(&links.first.token, Some(&grant)).await;
    let without = links.authorize(&links.first.token, None).await;
    let other_link = links.authorize(&links.second.token, Some(&grant)).await;
    let altered = links.authorize(&links.first.token, Some(&tampered)).await;
    let plain_with = links.authorize(&links.plain.token, Some(&grant)).await;
    let plain_without = links.authorize(&links.plain.token, None).await;
    harness.clock.tick(harness.config.grant_ttl - MICRO);
    let last_moment = links.authorize(&links.first.token, Some(&grant)).await;
    harness.clock.tick(MICRO);
    let expired = links.authorize(&links.first.token, Some(&grant)).await;

    // Assert
    let content = content_of(&links.owner, links.file);
    assert_eq!(own.expect("own link with its grant"), content);
    assert_eq!(last_moment.expect("before the expiry"), content);
    for refused in [without, other_link, altered, expired] {
        assert!(
            matches!(refused, Err(PublicShareError::PasswordRequired)),
            "{refused:?}"
        );
    }
    assert_eq!(plain_with.expect("plain with a grant"), content);
    assert_eq!(plain_without.expect("plain without"), content);
    links.finish(harness).await;
}

#[tokio::test]
async fn a_grant_is_stored_nowhere_and_ends_with_its_link() {
    // Arrange
    let harness = Harness::new().await;
    let links = Links::new(&harness).await;
    let (grant, _) = links.grants().await;
    let before = links.authorize(&links.first.token, Some(&grant)).await;
    links
        .shares
        .revoke(links.owner.account, links.first.id)
        .await
        .expect("revoke");

    // Act
    let after = links.authorize(&links.first.token, Some(&grant)).await;
    let mut admin = harness.db.admin().await;
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT tablename::text FROM pg_tables WHERE schemaname = 'public'")
            .fetch_all(&mut admin)
            .await
            .expect("list tables");
    let mut holding = Vec::new();
    for table in &tables {
        // The value is URL-safe base64, so it cannot break out of the literal.
        let rows = harness
            .db
            .admin_count(&format!(
                "SELECT count(*) FROM public.\"{table}\" t WHERE strpos(t::text, '{}') > 0",
                grant.value
            ))
            .await;
        if rows > 0 {
            holding.push(table.clone());
        }
    }
    admin.close().await.expect("close the admin connection");

    // Assert
    assert_eq!(
        before.expect("valid before the revoke"),
        content_of(&links.owner, links.file)
    );
    assert!(
        matches!(after, Err(PublicShareError::NotFound)),
        "{after:?}"
    );
    assert!(tables.iter().any(|table| table == "share_links"));
    // `device_grants` is 01's device authorization; no table of 06 holds access grants.
    assert!(
        !tables
            .iter()
            .any(|table| table.starts_with("share") && table != "share_links"),
        "{tables:?}"
    );
    assert_eq!(holding, Vec::<String>::new(), "no row holds the grant");
    links.finish(harness).await;
}

#[tokio::test]
async fn a_thousand_mutated_grants_are_refused_on_both_links() {
    // Arrange
    let harness = Harness::new().await;
    let links = Links::new(&harness).await;
    let (first, second) = links.grants().await;
    let cases = [
        (&links.first.token, mutants(&first, &second)),
        (&links.second.token, mutants(&second, &first)),
    ];

    // Act
    let mut accepted = Vec::new();
    let mut counted = 0;
    for (token, grants) in &cases {
        counted += grants.len();
        for grant in grants {
            let answer = links.authorize(token, Some(grant)).await;
            if !matches!(answer, Err(PublicShareError::PasswordRequired)) {
                accepted.push(format!("{grant:?} -> {answer:?}"));
            }
        }
    }
    let own = links.authorize(&links.first.token, Some(&first)).await;

    // Assert
    assert!(
        cases.iter().all(|(_, grants)| grants.len() >= 1000),
        "{counted} mutants"
    );
    assert_eq!(accepted, Vec::<String>::new());
    assert!(own.is_ok(), "the unchanged grant still opens its link");
    links.finish(harness).await;
}

#[tokio::test]
async fn a_near_valid_grant_is_refused_as_fast_as_an_invalid_one() {
    // Arrange
    let harness = Harness::new().await;
    let links = Links::new(&harness).await;
    let (grant, _) = links.grants().await;
    let mut last_changed = grant.value.clone().into_bytes();
    let last = last_changed.len() - 1;
    last_changed[last] = if last_changed[last] == b'A' {
        b'B'
    } else {
        b'A'
    };
    let near = with_value(&grant, String::from_utf8(last_changed).expect("ascii"));
    let invalid = with_value(&grant, "_".repeat(grant.value.len()));
    let rounds = 101;

    // Act
    let (mut near_times, mut invalid_times) = (Vec::new(), Vec::new());
    for _ in 0..rounds {
        for (candidate, times) in [(&near, &mut near_times), (&invalid, &mut invalid_times)] {
            let started = Instant::now();
            let answer = links.authorize(&links.first.token, Some(candidate)).await;
            times.push(started.elapsed());
            assert!(matches!(answer, Err(PublicShareError::PasswordRequired)));
        }
    }

    // Assert
    let (near_median, invalid_median) = (median(near_times), median(invalid_times));
    let gap = near_median.abs_diff(invalid_median);
    // Both answers include a database round trip that takes several milliseconds on a loaded runner; a skipped
    // comparison or hash would differ by far more than half of that.
    let tolerance = (invalid_median / 2).max(Duration::from_millis(1));
    assert!(
        gap <= tolerance,
        "near {near_median:?}, invalid {invalid_median:?}"
    );
    links.finish(harness).await;
}

#[tokio::test]
async fn open_answers_a_password_link_with_the_file_only_for_a_valid_grant_of_its_own() {
    // Arrange
    let harness = Harness::new().await;
    let links = Links::new(&harness).await;
    let (grant, other) = links.grants().await;
    let altered = with_value(&grant, "A".repeat(grant.value.len()));

    // Act
    let without = links.open(&links.first.token, None).await;
    let other_link = links.open(&links.first.token, Some(&other)).await;
    let tampered = links.open(&links.first.token, Some(&altered)).await;
    let valid = links.open(&links.first.token, Some(&grant)).await;
    harness.clock.tick(harness.config.grant_ttl);
    let expired = links.open(&links.first.token, Some(&grant)).await;

    // Assert
    for refused in [without, other_link, tampered, expired] {
        assert!(
            matches!(refused, Ok(ShareOpen::PasswordRequired)),
            "{refused:?}"
        );
    }
    let opened = valid.expect("open with the link's own grant");
    assert!(matches!(opened, ShareOpen::Available { .. }), "{opened:?}");
    links.finish(harness).await;
}

#[tokio::test]
async fn open_of_a_link_without_a_password_ignores_a_stray_grant() {
    // Arrange
    let harness = Harness::new().await;
    let links = Links::new(&harness).await;
    let (grant, _) = links.grants().await;

    // Act
    let plain_with = links.open(&links.plain.token, Some(&grant)).await;
    let plain_without = links.open(&links.plain.token, None).await;

    // Assert
    assert!(matches!(plain_with, Ok(ShareOpen::Available { .. })));
    assert_eq!(plain_with.ok(), plain_without.ok());
    links.finish(harness).await;
}
