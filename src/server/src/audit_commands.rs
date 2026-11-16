//! `admin audit export --since <time>` (E-22/c09): every row at or after the time as one JSON line on stdout,
//! page by page. A line holds ids, codes, integers and the time only; the stored client address is left out.

use sc_audit::{AuditReader, AuditRow, StoredValue};
use sc_error::ErrorCode;
use serde_json::{Map, Value};

use crate::error::CliError;
use crate::exit::Exit;
use crate::handler::Call;
use crate::outcome::Outcome;
use crate::utc_arg;

pub(crate) async fn export(since: &str, call: &Call<'_>) -> Result<Outcome, CliError> {
    let since = utc_arg::parse("--since", since)?;
    let reader = call.services.get::<AuditReader>().await?;
    let mut cursor = None;
    loop {
        let page = reader
            .export(since, None, cursor)
            .await
            .map_err(|error| failed(&error.to_string()))?;
        let mut text = String::new();
        for row in &page.rows {
            text.push_str(&line(row)?);
            text.push('\n');
        }
        call.terminal
            .write_stdout(&text)
            .map_err(|error| failed(&format!("cannot write the export: {error}")))?;
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(Outcome::success().document(""))
}

fn line(row: &AuditRow) -> Result<String, CliError> {
    let mut object = Map::new();
    object.insert("id".to_owned(), Value::from(row.id));
    object.insert("at_micros".to_owned(), Value::from(row.at.as_unix_micros()));
    object.insert("event".to_owned(), Value::from(row.event.as_str()));
    object.insert("actor_account_id".to_owned(), id(row.actor_account_id));
    object.insert("actor_session_id".to_owned(), id(row.actor_session_id));
    object.insert(
        "subject_type".to_owned(),
        Value::from(row.subject_type.clone()),
    );
    object.insert("subject_id".to_owned(), id(row.subject_id));
    if let Some(detail) = &row.detail {
        let values: Map<String, Value> = detail
            .iter()
            .map(|(key, value)| (key.clone(), stored(value)))
            .collect();
        object.insert("detail".to_owned(), Value::Object(values));
    }
    serde_json::to_string(&Value::Object(object))
        .map_err(|error| failed(&format!("cannot encode an audit row: {error}")))
}

fn id(value: Option<uuid::Uuid>) -> Value {
    Value::from(value.map(|id| id.to_string()))
}

fn stored(value: &StoredValue) -> Value {
    match value {
        StoredValue::Id(id) => Value::from(id.to_string()),
        StoredValue::Code(code) => Value::from(code.as_str()),
        StoredValue::Int(number) => Value::from(*number),
    }
}

fn failed(detail: &str) -> CliError {
    CliError::Operator {
        code: ErrorCode::Internal.as_str(),
        detail: format!("audit export: {detail}"),
        exit: Exit::Failed,
    }
}
