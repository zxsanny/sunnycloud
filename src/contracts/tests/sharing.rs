//! Recording doubles of the 06 sharing ports (AC-2) and the uniform not-found outcome (AC-3, AC-12).

mod common;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use common::{Calls, IP, NOW, fault, id, sorted};
use contracts::{
    AccessGrant, CreatedShare, LiveFile, NodeKind, PublicShareError, PublicShareService,
    ShareContent, ShareError, ShareInfo, ShareOpen, ShareService, async_trait,
};
use sc_ids::{RawToken, TokenKind};
use uuid::Uuid;

#[derive(Default)]
struct Double(Calls);

const SECRET: &str = "share-token-secret-AAAA1111";
const GRANT: &str = "grant-secret-BBBB2222";

fn token() -> RawToken {
    RawToken::from_presented(TokenKind::Share, SECRET.to_owned())
}

fn grant() -> AccessGrant {
    AccessGrant {
        value: GRANT.to_owned(),
        expires_at: NOW,
    }
}

fn folder() -> LiveFile {
    LiveFile {
        kind: NodeKind::Folder,
        revision: None,
        blob_ref: None,
    }
}

#[async_trait]
impl ShareService for Double {
    async fn create(
        &self,
        _account: Uuid,
        _node_id: Uuid,
        expires_in: Option<Duration>,
        password: Option<&str>,
    ) -> Result<CreatedShare, ShareError> {
        self.0.record("create");
        if expires_in.is_some_and(|d| d.is_zero()) {
            return Err(ShareError::ExpiryOutOfRange);
        }
        Ok(CreatedShare {
            id: id(),
            token: token(),
            url_path: "/s/x".to_owned(),
            expires_at: NOW,
            has_password: password.is_some(),
        })
    }

    async fn list(
        &self,
        _account: Uuid,
        _node_id: Option<Uuid>,
    ) -> Result<Vec<ShareInfo>, ShareError> {
        self.0.record("list");
        Ok(vec![ShareInfo {
            id: id(),
            node_id: id(),
            expires_at: NOW,
            has_password: false,
            created_at: NOW,
        }])
    }

    async fn revoke(&self, _account: Uuid, _share_id: Uuid) -> Result<(), ShareError> {
        self.0.record("revoke");
        Ok(())
    }
}

#[async_trait]
impl PublicShareService for Double {
    async fn open(
        &self,
        _token: &RawToken,
        _client_ip: IpAddr,
    ) -> Result<ShareOpen, PublicShareError> {
        self.0.record("open");
        Ok(ShareOpen::NotFound)
    }

    async fn verify_password(
        &self,
        _token: &RawToken,
        password: &str,
        _client_ip: IpAddr,
    ) -> Result<AccessGrant, PublicShareError> {
        self.0.record("verify_password");
        if password.is_empty() {
            return Err(PublicShareError::WrongPassword);
        }
        Ok(grant())
    }

    async fn authorize_content(
        &self,
        _token: &RawToken,
        grant: Option<&AccessGrant>,
        _client_ip: IpAddr,
        _starts_download: bool,
    ) -> Result<ShareContent, PublicShareError> {
        self.0.record("authorize_content");
        if grant.is_none() {
            return Err(PublicShareError::PasswordRequired);
        }
        Ok(ShareContent {
            account_id: id(),
            node_id: id(),
            revision_id: id(),
        })
    }

    async fn authorize_derived(
        &self,
        _token: &RawToken,
        _grant: Option<&AccessGrant>,
        _client_ip: IpAddr,
    ) -> Result<LiveFile, PublicShareError> {
        self.0.record("authorize_derived");
        Ok(folder())
    }
}

#[tokio::test]
async fn share_service_double_creates_lists_and_revokes() {
    // Arrange
    let calls = Calls::default();
    let shares: Arc<dyn ShareService> = Arc::new(Double(calls.clone()));

    // Act
    let created = shares
        .create(id(), id(), Some(Duration::from_secs(60)), Some("pw"))
        .await
        .expect("create");
    let listed = shares.list(id(), None).await.expect("list");
    shares.revoke(id(), created.id).await.expect("revoke");

    // Assert
    assert!(created.has_password);
    assert_eq!(listed.len(), 1);
    assert_eq!(calls.sorted(), sorted(vec!["create", "list", "revoke"]));
}

#[tokio::test]
async fn share_service_create_with_a_zero_expiry_is_out_of_range() {
    // Arrange
    let shares: Arc<dyn ShareService> = Arc::new(Double::default());

    // Act
    let outcome = shares.create(id(), id(), Some(Duration::ZERO), None).await;

    // Assert
    assert!(matches!(outcome, Err(ShareError::ExpiryOutOfRange)));
}

#[tokio::test]
async fn public_share_service_double_records_all_four_methods() {
    // Arrange
    let calls = Calls::default();
    let public: Arc<dyn PublicShareService> = Arc::new(Double(calls.clone()));
    let token = token();

    // Act
    let opened = public.open(&token, IP).await.expect("open");
    let verified = public
        .verify_password(&token, "pw", IP)
        .await
        .expect("verify");
    let content = public
        .authorize_content(&token, Some(&verified), IP, true)
        .await
        .expect("content");
    let derived = public
        .authorize_derived(&token, None, IP)
        .await
        .expect("derived");

    // Assert
    assert_eq!(opened, ShareOpen::NotFound);
    assert_eq!(content.account_id, id());
    assert_eq!(derived.kind, NodeKind::Folder);
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "open",
            "verify_password",
            "authorize_content",
            "authorize_derived"
        ])
    );
}

#[tokio::test]
async fn public_share_service_authorize_content_without_a_grant_requires_the_password() {
    // Arrange
    let public: Arc<dyn PublicShareService> = Arc::new(Double::default());

    // Act
    let outcome = public.authorize_content(&token(), None, IP, false).await;

    // Assert
    assert!(matches!(outcome, Err(PublicShareError::PasswordRequired)));
}

fn open_name(open: &ShareOpen) -> &'static str {
    match open {
        ShareOpen::Available { .. } => "available",
        ShareOpen::PasswordRequired => "password_required",
        ShareOpen::NotFound => "not_found",
    }
}

#[test]
fn share_open_has_one_not_found_variant_and_it_carries_no_reason() {
    // Arrange
    let missing = ShareOpen::NotFound;

    // Act
    let name = open_name(&missing);

    // Assert
    assert_eq!(name, "not_found");
    assert_eq!(format!("{missing:?}"), "NotFound");
}

fn public_error_name(error: &PublicShareError) -> &'static str {
    match error {
        PublicShareError::NotFound => "not_found",
        PublicShareError::WrongPassword => "wrong_password",
        PublicShareError::PasswordRequired => "password_required",
        PublicShareError::RateLimited { .. } => "rate_limited",
        PublicShareError::Unexpected(_) => "unexpected",
    }
}

#[test]
fn public_share_error_has_one_not_found_variant_and_the_listed_others() {
    // Arrange
    let all = [
        PublicShareError::NotFound,
        PublicShareError::WrongPassword,
        PublicShareError::PasswordRequired,
        PublicShareError::RateLimited {
            retry_after: Duration::from_secs(5),
        },
        PublicShareError::Unexpected(fault()),
    ];

    // Act
    let names: Vec<_> = all.iter().map(public_error_name).collect();

    // Assert
    assert_eq!(
        names,
        [
            "not_found",
            "wrong_password",
            "password_required",
            "rate_limited",
            "unexpected"
        ]
    );
}

#[test]
fn a_created_share_and_an_access_grant_print_placeholders() {
    // Arrange
    let created = CreatedShare {
        id: id(),
        token: token(),
        url_path: "/s/x".to_owned(),
        expires_at: NOW,
        has_password: false,
    };

    // Act
    let printed = format!("{created:?} {:?}", grant());

    // Assert
    assert!(printed.contains("<redacted>"), "{printed}");
    assert!(!printed.contains(SECRET), "{printed}");
    assert!(!printed.contains(GRANT), "{printed}");
}
