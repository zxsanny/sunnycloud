use std::net::IpAddr;

use sc_clock::Timestamp;
use sc_db::{Db, DbError};
use serde_json::Value;
use uuid::Uuid;

use crate::detail::is_token;
use crate::error::AuditError;
use crate::event::AuditEventKind;
use crate::repo::{self, RawRow};

/// Rows per exported page.
pub const EXPORT_PAGE_ROWS: usize = 500;

/// Where the next page starts. Opaque: take it from [`AuditPage::next`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageCursor(i64);

/// A stored detail value: an id, an enumeration code or an integer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredValue {
    Id(Uuid),
    Code(String),
    Int(i64),
}

/// A stored row. Every value is an id, a code, an address, a timestamp or an integer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub id: i64,
    pub at: Timestamp,
    pub event: AuditEventKind,
    pub actor_account_id: Option<Uuid>,
    pub actor_session_id: Option<Uuid>,
    pub subject_type: Option<String>,
    pub subject_id: Option<Uuid>,
    pub ip: Option<IpAddr>,
    pub detail: Option<Vec<(String, StoredValue)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditPage {
    pub rows: Vec<AuditRow>,
    /// `None` on the last page.
    pub next: Option<PageCursor>,
}

/// Read side for the admin export (`sunnycloud admin audit export`, the admin audit screen).
#[derive(Clone)]
pub struct AuditReader {
    db: Db,
}

impl AuditReader {
    pub fn new(db: Db) -> Self {
        AuditReader { db }
    }

    /// Up to [`EXPORT_PAGE_ROWS`] rows at or after `since`, in insertion order, optionally of one event kind.
    /// Pass the previous page's `next` to continue; every matching row is returned exactly once.
    pub async fn export(
        &self,
        since: Timestamp,
        event: Option<AuditEventKind>,
        cursor: Option<PageCursor>,
    ) -> Result<AuditPage, AuditError> {
        let after = cursor.map_or(0, |cursor| cursor.0);
        let limit = i64::try_from(EXPORT_PAGE_ROWS + 1).unwrap_or(i64::MAX);
        let mut raw = repo::unscoped(&self.db, async |tx| {
            repo::page(tx, since, event, after, limit).await
        })
        .await?;
        let more = raw.len() > EXPORT_PAGE_ROWS;
        raw.truncate(EXPORT_PAGE_ROWS);
        let rows = raw
            .into_iter()
            .map(row_from)
            .collect::<Result<Vec<_>, _>>()?;
        let next = if more {
            rows.last().map(|row| PageCursor(row.id))
        } else {
            None
        };
        Ok(AuditPage { rows, next })
    }
}

fn row_from(raw: RawRow) -> Result<AuditRow, AuditError> {
    let (
        id,
        micros,
        event,
        actor_account_id,
        actor_session_id,
        subject_type,
        subject_id,
        ip,
        detail,
    ) = raw;
    let event = AuditEventKind::from_code(&event)
        .ok_or_else(|| corrupt("an event outside the 14 kinds"))?;
    if subject_type.as_deref().is_some_and(|text| !is_token(text)) {
        return Err(corrupt("a subject type that is not an identifier"));
    }
    let ip = ip
        .map(|text| text.parse::<IpAddr>())
        .transpose()
        .map_err(|_| corrupt("an address that does not parse"))?;
    Ok(AuditRow {
        id,
        at: Timestamp::from_unix_micros(micros),
        event,
        actor_account_id,
        actor_session_id,
        subject_type,
        subject_id,
        ip,
        detail: detail.as_deref().map(stored_detail).transpose()?,
    })
}

fn stored_detail(text: &str) -> Result<Vec<(String, StoredValue)>, AuditError> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
        return Err(corrupt("a detail that is not an object"));
    };
    map.into_iter()
        .map(|(key, value)| {
            if !is_token(&key) {
                return Err(corrupt("a detail key that is not an identifier"));
            }
            Ok((key, stored_value(&value)?))
        })
        .collect()
}

fn stored_value(value: &Value) -> Result<StoredValue, AuditError> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .map(StoredValue::Int)
            .ok_or_else(|| corrupt("a detail number that is not an integer")),
        Value::String(text) if is_token(text) => Ok(StoredValue::Code(text.clone())),
        Value::String(text) if text.len() == 36 => Uuid::parse_str(text)
            .map(StoredValue::Id)
            .map_err(|_| corrupt("a detail text that is neither an id nor a code")),
        _ => Err(corrupt(
            "a detail value that is not an id, a code or an integer",
        )),
    }
}

// A row the writer could not have produced: reported as a store failure, never returned as text.
fn corrupt(what: &'static str) -> AuditError {
    AuditError::Store(DbError::Other(format!("stored audit row has {what}")))
}
