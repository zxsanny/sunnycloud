//! `report_unlock_failure` (AZ-3924, ST-20-2): one `vault_failure` audit row and one audit log line per report of
//! an own vault, holding account id, vault id and time only; nothing for an unknown or foreign vault.

use contracts::{VaultError, VaultService};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::harness::START;
use crate::logs::LogCapture;
use crate::service_support::Env;

const REPORTS: usize = 20;

/// One `audit_log` row: at (microseconds), event, actor account, actor session, subject type, subject id, ip,
/// detail.
type AuditRow = (
    i64,
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
);

async fn audit_rows(probe: &mut PgConnection) -> Vec<AuditRow> {
    sqlx::query_as(
        "SELECT (extract(epoch FROM at) * 1000000)::bigint, event, actor_account_id, actor_session_id, \
         subject_type, subject_id, host(ip), detail::text FROM audit_log ORDER BY id",
    )
    .fetch_all(probe)
    .await
    .expect("read audit_log")
}

#[tokio::test]
async fn report_unlock_failure_own_vault_twenty_reports_store_twenty_id_only_rows_and_lines() {
    // Arrange
    let mut env = Env::new().await;
    let (account, vault, _) = env.account_with_vault(b"header").await;
    let logs = LogCapture::install();

    // Act
    for _ in 0..REPORTS {
        env.service
            .report_unlock_failure(account, vault)
            .await
            .expect("report unlock failure");
    }

    // Assert
    let expected: AuditRow = (
        START.as_unix_micros(),
        "vault_failure".to_owned(),
        Some(account),
        None,
        Some("vault".to_owned()),
        Some(vault),
        None,
        None,
    );
    assert_eq!(audit_rows(&mut env.probe).await, vec![expected; REPORTS]);
    let lines = logs.audit_lines();
    assert_eq!(lines.len(), REPORTS, "{lines:?}");
    for line in lines {
        let fields: Vec<(&str, &str)> = line
            .fields
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let (account, vault) = (account.to_string(), vault.to_string());
        assert_eq!(
            fields,
            [
                ("actor_account_id", account.as_str()),
                ("event", "vault_failure"),
                ("message", "audit"),
                ("subject_id", vault.as_str()),
                ("subject_type", "vault"),
            ]
        );
    }
    env.finish().await;
}

#[tokio::test]
async fn report_unlock_failure_foreign_or_unknown_vault_is_not_found_and_writes_nothing() {
    // Arrange
    let mut env = Env::new().await;
    let (caller, _) = env.account().await;
    let (_, foreign_vault, _) = env.account_with_vault(b"header").await;
    let unknown_vault = Uuid::new_v4();
    let logs = LogCapture::install();

    // Act
    let foreign = env
        .service
        .report_unlock_failure(caller, foreign_vault)
        .await;
    let unknown = env
        .service
        .report_unlock_failure(caller, unknown_vault)
        .await;

    // Assert
    assert!(matches!(foreign, Err(VaultError::NotFound)), "{foreign:?}");
    assert!(matches!(unknown, Err(VaultError::NotFound)), "{unknown:?}");
    assert_eq!(audit_rows(&mut env.probe).await, Vec::<AuditRow>::new());
    assert!(logs.audit_lines().is_empty(), "{:?}", logs.audit_lines());
    env.finish().await;
}
