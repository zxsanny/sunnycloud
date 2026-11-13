//! Recording double of the 20 vault port (AC-2).

mod common;

use std::sync::Arc;

use common::{Calls, fault, id, sorted};
use contracts::{CreatedVault, ETag, VaultError, VaultHeader, VaultService, async_trait};
use uuid::Uuid;

#[derive(Default)]
struct Double(Calls);

fn etag(text: &str) -> ETag {
    ETag(text.to_owned())
}

#[async_trait]
impl VaultService for Double {
    async fn create_vault(
        &self,
        _account: Uuid,
        header: &[u8],
        _root_node_id: Uuid,
    ) -> Result<CreatedVault, VaultError> {
        self.0.record("create_vault");
        if header.len() > 8 {
            return Err(VaultError::HeaderTooLarge);
        }
        Ok(CreatedVault {
            vault_id: id(),
            etag: etag("v1"),
        })
    }

    async fn get_header(&self, _account: Uuid, _vault_id: Uuid) -> Result<VaultHeader, VaultError> {
        self.0.record("get_header");
        Ok(VaultHeader {
            header: vec![1, 2, 3],
            etag: etag("v1"),
            root_node_id: id(),
        })
    }

    async fn replace_header(
        &self,
        _account: Uuid,
        _vault_id: Uuid,
        _header: &[u8],
        if_match: &ETag,
    ) -> Result<ETag, VaultError> {
        self.0.record("replace_header");
        if *if_match != etag("v1") {
            return Err(VaultError::PreconditionFailed);
        }
        Ok(etag("v2"))
    }

    async fn report_unlock_failure(
        &self,
        _account: Uuid,
        _vault_id: Uuid,
    ) -> Result<(), VaultError> {
        self.0.record("report_unlock_failure");
        Ok(())
    }
}

#[tokio::test]
async fn vault_service_double_records_every_method() {
    // Arrange
    let calls = Calls::default();
    let vaults: Arc<dyn VaultService> = Arc::new(Double(calls.clone()));

    // Act
    let created = vaults.create_vault(id(), &[1], id()).await.expect("create");
    let read = vaults
        .get_header(id(), created.vault_id)
        .await
        .expect("get");
    let replaced = vaults
        .replace_header(id(), created.vault_id, &[2], &read.etag)
        .await
        .expect("replace");
    vaults
        .report_unlock_failure(id(), created.vault_id)
        .await
        .expect("report");

    // Assert
    assert_eq!(replaced, etag("v2"));
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "create_vault",
            "get_header",
            "replace_header",
            "report_unlock_failure"
        ])
    );
}

#[tokio::test]
async fn vault_service_replace_with_a_stale_tag_is_a_precondition_failure() {
    // Arrange
    let vaults: Arc<dyn VaultService> = Arc::new(Double::default());

    // Act
    let outcome = vaults
        .replace_header(id(), id(), &[2], &etag("stale"))
        .await;

    // Assert
    assert!(matches!(outcome, Err(VaultError::PreconditionFailed)));
}

#[tokio::test]
async fn vault_service_create_with_an_oversized_header_is_refused() {
    // Arrange
    let vaults: Arc<dyn VaultService> = Arc::new(Double::default());

    // Act
    let outcome = vaults.create_vault(id(), &[0; 9], id()).await;

    // Assert
    assert!(matches!(outcome, Err(VaultError::HeaderTooLarge)));
}

fn vault_error_name(error: &VaultError) -> &'static str {
    match error {
        VaultError::VaultExists => "vault_exists",
        VaultError::HeaderTooLarge => "header_too_large",
        VaultError::NotFound => "not_found",
        VaultError::PreconditionFailed => "precondition_failed",
        VaultError::Unexpected(_) => "unexpected",
    }
}

#[test]
fn vault_error_has_the_contract_variants_and_the_tier_three_one() {
    // Arrange
    let all = [
        VaultError::VaultExists,
        VaultError::HeaderTooLarge,
        VaultError::NotFound,
        VaultError::PreconditionFailed,
        VaultError::Unexpected(fault()),
    ];

    // Act
    let names: Vec<_> = all.iter().map(vault_error_name).collect();

    // Assert
    assert_eq!(
        names,
        [
            "vault_exists",
            "header_too_large",
            "not_found",
            "precondition_failed",
            "unexpected"
        ]
    );
}
