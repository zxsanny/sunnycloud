//! AZ-3926 AC-5 (ST-20-2) against the real `sunnycloud serve` binary of a `vault` build on PostgreSQL 18: an
//! unlock-failure report whose body carries a passphrase-shaped string and a 64-byte key-shaped value answers 204,
//! its audit row holds ids and time only, and the body text is in no database row and no log line.

mod root_support;
mod serve_support;
mod vault_support;

#[cfg(feature = "vault")]
mod v12 {
    use serde_json::json;
    use sqlx::Connection;

    use crate::vault_support::{Running, json_type};

    /// event, actor account, subject type, subject id, detail.
    type AuditRow = (
        String,
        Option<uuid::Uuid>,
        Option<String>,
        Option<uuid::Uuid>,
        Option<String>,
    );

    const PASSPHRASE: &str = "correct-horse-battery-staple-7f3a-unlock-secret";

    /// 64 bytes as 128 hex digits, the shape of a raw vault key.
    fn key_hex() -> String {
        (0u8..64)
            .map(|byte| format!("{:02x}", byte.wrapping_mul(37).wrapping_add(11)))
            .collect()
    }

    fn hex(text: &str) -> String {
        text.bytes().map(|byte| format!("{byte:02x}")).collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_report_with_secret_shaped_body_is_204_and_the_body_is_in_no_row_and_no_log_line() {
        // Arrange
        let running = Running::start().await;
        let account = running.stack.account().await;
        let (vault, _) = running.create_vault(&account, b"header");
        let key = key_hex();
        let body = json!({"passphrase": PASSPHRASE, "key": key}).to_string();
        let path = format!("/api/v1/vaults/{vault}/unlock-failures");

        // Act
        let answer = running.as_account(&account, "POST", &path, &[json_type()], body.as_bytes());

        // Assert
        assert_eq!(answer.status, 204, "{answer:?}");
        assert!(answer.body.is_empty());
        let mut admin = running.stack.root.db.admin().await;
        let audit: Vec<AuditRow> = sqlx::query_as(
            "SELECT event, actor_account_id, subject_type, subject_id, detail::text FROM audit_log \
                 WHERE event = 'vault_failure'",
        )
        .fetch_all(&mut admin)
        .await
        .expect("audit rows");
        admin.close().await.expect("close admin connection");
        assert_eq!(
            audit,
            [(
                "vault_failure".to_owned(),
                Some(account.id),
                Some("vault".to_owned()),
                Some(vault),
                None
            )]
        );
        let needles = [
            PASSPHRASE.to_owned(),
            key.clone(),
            hex(PASSPHRASE),
            "correct-horse".to_owned(),
            key[..32].to_owned(),
        ];
        for needle in &needles {
            assert_eq!(
                running.rows_containing(needle).await,
                Vec::<String>::new(),
                "{needle}"
            );
        }
        let lines = running.finish().await;
        assert!(!lines.is_empty());
        for needle in &needles {
            let leaked: Vec<&String> = lines
                .iter()
                .filter(|line| line.contains(needle.as_str()))
                .collect();
            assert!(leaked.is_empty(), "{needle} in {leaked:?}");
        }
    }
}
