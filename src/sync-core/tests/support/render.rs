//! Turns a scripted [`Answer`] into the status, headers and body bytes of one response.

use bytes::Bytes;
use serde_json::{Value, json};
use uuid::Uuid;

use super::answer::{Answer, Payload};

pub struct Rendered {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

/// `range` is the request's `Range` header; `catcher_origin` replaces `{catcher}` in header values.
pub fn render(
    answer: &Answer,
    range: Option<&str>,
    request_id: Uuid,
    catcher_origin: &str,
) -> Rendered {
    let mut headers = vec![
        ("x-request-id".to_owned(), request_id.to_string()),
        ("x-content-type-options".to_owned(), "nosniff".to_owned()),
        ("cache-control".to_owned(), "no-store".to_owned()),
    ];
    let (status, body) = match &answer.payload {
        Payload::Empty => (answer.status, Bytes::new()),
        Payload::Bytes { content_type, data } => {
            headers.push(("content-type".to_owned(), content_type.clone()));
            (answer.status, flipped(data, 0, answer.flip))
        }
        Payload::Content(data) => content(data, range, answer.flip, &mut headers),
        Payload::Problem(problem) => {
            headers.push((
                "content-type".to_owned(),
                "application/problem+json".to_owned(),
            ));
            (answer.status, problem_body(problem, request_id))
        }
    };
    headers.extend(answer.headers.iter().map(|(name, value)| {
        (
            name.to_ascii_lowercase(),
            value.replace("{catcher}", catcher_origin),
        )
    }));
    Rendered {
        status,
        headers,
        body,
    }
}

fn problem_body(problem: &super::answer::Problem, request_id: Uuid) -> Bytes {
    let mut fields = serde_json::Map::new();
    fields.insert(
        "type".to_owned(),
        json!(format!("urn:sunnycloud:error:{}", problem.code)),
    );
    fields.insert("title".to_owned(), json!(problem.title));
    fields.insert("status".to_owned(), json!(problem.status));
    fields.insert("code".to_owned(), json!(problem.code));
    fields.insert("request_id".to_owned(), json!(request_id.to_string()));
    fields.extend(problem.extras.clone());
    Bytes::from(serde_json::to_vec(&Value::Object(fields)).expect("problem body serialises"))
}

fn content(
    data: &Bytes,
    range: Option<&str>,
    flip: Option<(usize, u8)>,
    headers: &mut Vec<(String, String)>,
) -> (u16, Bytes) {
    let total = data.len();
    headers.push(("accept-ranges".to_owned(), "bytes".to_owned()));
    headers.push(("etag".to_owned(), format!("\"fixture-{total:x}\"")));
    headers.push((
        "content-type".to_owned(),
        "application/octet-stream".to_owned(),
    ));
    headers.push(("cache-control".to_owned(), "private".to_owned()));
    match range.and_then(|text| parse_range(text, total)) {
        None => (200, flipped(data, 0, flip)),
        Some(Err(())) => {
            headers.push(("content-range".to_owned(), format!("bytes */{total}")));
            (416, Bytes::new())
        }
        Some(Ok((start, end))) => {
            headers.push((
                "content-range".to_owned(),
                format!("bytes {start}-{}/{total}", end - 1),
            ));
            (206, flipped(&data.slice(start..end), start, flip))
        }
    }
}

/// `Some(Ok((start, end_exclusive)))` for a satisfiable single range, `Some(Err(()))` for an unsatisfiable
/// one, `None` when the header is not a single `bytes=` range (served as a full 200).
fn parse_range(text: &str, total: usize) -> Option<Result<(usize, usize), ()>> {
    let spec = text.strip_prefix("bytes=")?;
    let (first, last) = spec.split_once('-')?;
    if spec.contains(',') {
        return None;
    }
    let (start, end) = match (first.is_empty(), last.is_empty()) {
        (true, true) => return None,
        (true, false) => {
            let suffix: usize = last.parse().ok()?;
            (total.saturating_sub(suffix), total)
        }
        (false, true) => (first.parse().ok()?, total),
        (false, false) => {
            let end: usize = last.parse().ok()?;
            (first.parse().ok()?, end.saturating_add(1).min(total))
        }
    };
    Some(if start >= total || start >= end {
        Err(())
    } else {
        Ok((start, end))
    })
}

/// `data` starts at byte `base` of the resource; the flip offset is a resource offset.
fn flipped(data: &Bytes, base: usize, flip: Option<(usize, u8)>) -> Bytes {
    let Some((offset, bit)) = flip else {
        return data.clone();
    };
    match offset.checked_sub(base).filter(|at| *at < data.len()) {
        None => data.clone(),
        Some(at) => {
            let mut copy = data.to_vec();
            copy[at] ^= 1 << (bit % 8);
            Bytes::from(copy)
        }
    }
}
