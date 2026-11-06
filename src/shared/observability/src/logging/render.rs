use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use super::LogFormat;
use super::event::{AUDIT_TARGET, Fields};

struct Extra<'a>(&'a [(&'static str, Value)]);

impl Serialize for Extra<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, value) in self.0 {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

/// The JSON line. Field order is the order of this struct; `request_id` is `null` outside a request.
#[derive(Serialize)]
struct JsonLine<'a> {
    ts: &'a str,
    level: &'static str,
    target: &'a str,
    msg: &'a str,
    request_id: Option<&'a str>,
    component: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    err: Option<&'a str>,
    #[serde(flatten)]
    extra: Extra<'a>,
}

pub(crate) struct Head<'a> {
    pub(crate) ts: &'a str,
    pub(crate) level: &'static str,
    pub(crate) target: &'a str,
    pub(crate) component: &'static str,
}

/// One line, without the trailing newline. The serializer cannot fail on these types; the fallback line
/// says so instead of dropping the event.
pub(crate) fn line(format: LogFormat, head: &Head<'_>, fields: &Fields) -> String {
    match format {
        LogFormat::Json => json(head, fields),
        LogFormat::Text => text(head, fields),
    }
}

fn json(head: &Head<'_>, fields: &Fields) -> String {
    let line = JsonLine {
        ts: head.ts,
        level: head.level,
        target: head.target,
        msg: &fields.msg,
        request_id: fields.request_id.as_deref(),
        component: head.component,
        stream: (head.target == AUDIT_TARGET).then_some("audit"),
        account_id: fields.account_id.as_deref(),
        node_id: fields.node_id.as_deref(),
        route: fields.route.as_deref(),
        status: fields.status,
        duration_ms: fields.duration_ms,
        err: fields.err.as_deref(),
        extra: Extra(&fields.audit),
    };
    serde_json::to_string(&line).unwrap_or_else(|_| {
        format!(
            r#"{{"ts":"{}","level":"error","target":"sc_observability","msg":"log line not serializable","request_id":null,"component":"{}"}}"#,
            head.ts, head.component
        )
    })
}

fn text(head: &Head<'_>, fields: &Fields) -> String {
    let mut out = format!(
        "{} {:<5} {} [{}] {}",
        head.ts,
        head.level.to_ascii_uppercase(),
        head.target,
        head.component,
        one_line(&fields.msg)
    );
    let pairs = [
        ("request_id", fields.request_id.as_deref()),
        ("account_id", fields.account_id.as_deref()),
        ("node_id", fields.node_id.as_deref()),
        ("route", fields.route.as_deref()),
    ];
    for (name, value) in pairs {
        if let Some(value) = value {
            push_pair(&mut out, name, value);
        }
    }
    if let Some(status) = fields.status {
        push_pair(&mut out, "status", &status.to_string());
    }
    if let Some(duration) = fields.duration_ms {
        push_pair(&mut out, "duration_ms", &duration.to_string());
    }
    if let Some(err) = &fields.err {
        push_pair(&mut out, "err", &format!("{:?}", one_line(err)));
    }
    for (name, value) in &fields.audit {
        push_pair(&mut out, name, &value.to_string());
    }
    out
}

fn push_pair(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push('=');
    out.push_str(value);
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}
