//! Reads of `audit_log` as the schema owner, and the switch that makes every audit insert of the API role fail
//! (the API role loses `INSERT` on the table, as the `sc_audit` tests force a store failure).

use sqlx::PgConnection;
use uuid::Uuid;

/// One stored event: kind, actor account, actor session, subject type, subject id, address, detail text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub event: String,
    pub actor_account_id: Option<Uuid>,
    pub actor_session_id: Option<Uuid>,
    pub subject_type: Option<String>,
    pub subject_id: Option<Uuid>,
    pub ip: Option<String>,
    pub detail: Option<String>,
}

type Tuple = (
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
);

pub async fn rows(conn: &mut PgConnection) -> Vec<Row> {
    let rows: Vec<Tuple> = sqlx::query_as(
        "SELECT event, actor_account_id, actor_session_id, subject_type, subject_id, host(ip), detail::text \
         FROM audit_log ORDER BY id",
    )
    .fetch_all(conn)
    .await
    .expect("read audit_log");
    rows.into_iter()
        .map(|row| Row {
            event: row.0,
            actor_account_id: row.1,
            actor_session_id: row.2,
            subject_type: row.3,
            subject_id: row.4,
            ip: row.5,
            detail: row.6,
        })
        .collect()
}

/// The stored rows of `event`.
pub async fn of(conn: &mut PgConnection, event: &str) -> Vec<Row> {
    rows(conn)
        .await
        .into_iter()
        .filter(|row| row.event == event)
        .collect()
}

/// Every later audit insert of the API role fails with a store error.
pub async fn block_inserts(conn: &mut PgConnection) {
    sqlx::query("REVOKE INSERT ON audit_log FROM sunnycloud")
        .execute(conn)
        .await
        .expect("revoke the audit insert");
}
