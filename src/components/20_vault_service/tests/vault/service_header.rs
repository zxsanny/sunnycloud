//! Header service (AZ-3923) on a real PostgreSQL 18: AC-1 (byte-identical round trip, ETag = SHA-256 of the stored
//! bytes, also after a restart), AC-2 (header cap), AC-4 (precondition on replace) and AC-6 (account scope).
//! The expected digests were computed outside the product (Python `hashlib.sha256`).

use contracts::{ETag, VaultError, VaultService};
use uuid::Uuid;

use crate::service_support::{Env, MAX_HEADER, service_on};

const SHA256_A: &str = "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb";
const SHA256_512: &str = "c9d8e3352f9f790d8b0be13cb1c18ed7963009888be04acc065ee5efbd934076";
const SHA256_ALL_BYTES: &str = "40aff2e9d2d8922e47afd4648e6967497158785fbd1da870e7110266bf944880";
const SHA256_MAX: &str = "9009d83ef59bc6ee9cd21887aeeb25a56c84490e0bc8256c4e52abda6515a857";
const SHA256_H1: &str = "8bf797aa02bd65b7cbd52155f6e010be77bb3d461320dac4c74c12d749d81066";

/// `(header, its SHA-256)`: 1 byte, 512 bytes, the 256 byte values, and exactly the cap.
fn headers() -> Vec<(Vec<u8>, &'static str)> {
    let pattern = |len: usize, step: usize, offset: usize| -> Vec<u8> {
        (0..len)
            .map(|i| ((i * step + offset) % 256) as u8)
            .collect()
    };
    vec![
        (b"a".to_vec(), SHA256_A),
        (pattern(512, 7, 3), SHA256_512),
        ((0..=255u8).collect(), SHA256_ALL_BYTES),
        (pattern(MAX_HEADER, 13, 5), SHA256_MAX),
    ]
}

fn etag(hex: &str) -> ETag {
    ETag(hex.to_owned())
}

#[tokio::test]
async fn create_then_get_returns_the_same_bytes_and_their_sha256_before_and_after_a_restart() {
    // Arrange
    let mut env = Env::new().await;
    let mut created = Vec::new();
    for (header, digest) in headers() {
        let (account, folder) = env.account().await;
        let vault = env
            .service
            .create_vault(account, &header, folder)
            .await
            .expect("create vault");
        assert_eq!(vault.etag, etag(digest), "create of {} bytes", header.len());
        created.push((account, vault.vault_id, folder, header, digest));
    }

    // Act
    let mut before = Vec::new();
    for (account, vault, ..) in &created {
        before.push(env.service.get_header(*account, *vault).await);
    }
    let restarted_db = env.harness.db.api().await;
    let restarted = service_on(&restarted_db);
    let mut after = Vec::new();
    for (account, vault, ..) in &created {
        after.push(restarted.get_header(*account, *vault).await);
    }

    // Assert
    for (((_, _, folder, header, digest), first), second) in created.iter().zip(before).zip(after) {
        for read in [first, second] {
            let read = read.expect("get header");
            assert_eq!(&read.header, header, "{} bytes", header.len());
            assert_eq!(read.etag, etag(digest), "{} bytes", header.len());
            assert_eq!(read.root_node_id, *folder);
        }
    }
    drop((restarted, restarted_db));
    env.finish().await;
}

#[tokio::test]
async fn create_with_one_byte_over_the_cap_is_header_too_large_and_stores_nothing() {
    // Arrange
    let mut env = Env::new().await;
    let (account, folder) = env.account().await;
    let over = vec![7u8; MAX_HEADER + 1];

    // Act
    let created = env.service.create_vault(account, &over, folder).await;

    // Assert
    assert!(
        matches!(created, Err(VaultError::HeaderTooLarge)),
        "{created:?}"
    );
    assert_eq!(env.vaults_of(account).await, 0);
    env.finish().await;
}

#[tokio::test]
async fn replace_with_one_byte_over_the_cap_is_header_too_large_and_changes_nothing() {
    // Arrange
    let mut env = Env::new().await;
    let (account, vault, current) = env.account_with_vault(b"H0").await;
    let over = vec![7u8; MAX_HEADER + 1];

    // Act
    let replaced = env
        .service
        .replace_header(account, vault, &over, &current)
        .await;

    // Assert
    assert!(
        matches!(replaced, Err(VaultError::HeaderTooLarge)),
        "{replaced:?}"
    );
    assert_eq!(env.stored(vault).await, Some(b"H0".to_vec()));
    env.finish().await;
}

#[tokio::test]
async fn replace_with_a_stale_etag_fails_and_with_the_current_etag_stores_the_new_header() {
    // Arrange
    let mut env = Env::new().await;
    let (account, vault, e0) = env.account_with_vault(b"H0").await;
    let stale = etag(SHA256_A);

    // Act
    let refused = env
        .service
        .replace_header(account, vault, b"stale", &stale)
        .await;
    let kept = env.stored(vault).await;
    let replaced = env.service.replace_header(account, vault, b"H1", &e0).await;

    // Assert
    assert!(
        matches!(refused, Err(VaultError::PreconditionFailed)),
        "{refused:?}"
    );
    assert_eq!(kept, Some(b"H0".to_vec()));
    assert_eq!(replaced.expect("replace with E0"), etag(SHA256_H1));
    assert_eq!(env.stored(vault).await, Some(b"H1".to_vec()));
    env.finish().await;
}

#[tokio::test]
async fn replace_with_a_malformed_or_differently_cased_etag_fails_and_keeps_the_header() {
    // Arrange
    let mut env = Env::new().await;
    let (account, vault, e0) = env.account_with_vault(b"H0").await;
    let candidates = [
        ETag(String::new()),
        ETag("not-a-digest".to_owned()),
        ETag(format!("\"{}\"", e0.0)),
        ETag(e0.0.to_uppercase()),
    ];

    // Act
    let mut outcomes = Vec::new();
    for candidate in &candidates {
        outcomes.push(
            env.service
                .replace_header(account, vault, b"H1", candidate)
                .await,
        );
    }

    // Assert
    for (candidate, outcome) in candidates.iter().zip(outcomes) {
        assert!(
            matches!(outcome, Err(VaultError::PreconditionFailed)),
            "{candidate:?}: {outcome:?}"
        );
    }
    assert_eq!(env.stored(vault).await, Some(b"H0".to_vec()));
    env.finish().await;
}

#[tokio::test]
async fn replace_to_the_same_bytes_keeps_the_etag() {
    // Arrange
    let mut env = Env::new().await;
    let (account, vault, e0) = env.account_with_vault(b"H0").await;

    // Act
    let replaced = env.service.replace_header(account, vault, b"H0", &e0).await;

    // Assert
    assert_eq!(replaced.expect("replace"), e0);
    env.finish().await;
}

#[tokio::test]
async fn another_account_gets_not_found_on_get_and_replace_like_an_unknown_id() {
    // Arrange
    let mut env = Env::new().await;
    let (_, vault_of_a, etag_of_a) = env.account_with_vault(b"H0").await;
    let (b, _) = env.account().await;
    let unknown = Uuid::new_v4();

    // Act
    let read = env.service.get_header(b, vault_of_a).await;
    let replaced = env
        .service
        .replace_header(b, vault_of_a, b"B", &etag_of_a)
        .await;
    let read_unknown = env.service.get_header(b, unknown).await;
    let replaced_unknown = env
        .service
        .replace_header(b, unknown, b"B", &etag_of_a)
        .await;

    // Assert
    for outcome in [
        read.map(|_| ()),
        replaced.map(|_| ()),
        read_unknown.map(|_| ()),
    ] {
        assert!(matches!(outcome, Err(VaultError::NotFound)), "{outcome:?}");
    }
    assert!(
        matches!(replaced_unknown, Err(VaultError::NotFound)),
        "{replaced_unknown:?}"
    );
    assert_eq!(env.stored(vault_of_a).await, Some(b"H0".to_vec()));
    env.finish().await;
}

#[tokio::test]
async fn report_unlock_failure_is_not_built_by_this_service_yet() {
    // Arrange
    let mut env = Env::new().await;
    let (account, vault, _) = env.account_with_vault(b"H0").await;

    // Act
    let reported = env.service.report_unlock_failure(account, vault).await;

    // Assert
    assert!(
        matches!(reported, Err(VaultError::Unexpected(_))),
        "{reported:?}"
    );
    env.finish().await;
}
