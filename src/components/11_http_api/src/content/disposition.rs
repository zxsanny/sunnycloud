//! `Content-Type`, `Content-Disposition` and the attachment-only headers of one download.

use axum::http::{HeaderMap, HeaderValue, header};
use contracts::Disposition;

use super::InconsistentDownload;

/// The only types served inline (AC-47), as `DownloadService` sniffs them.
const INLINE_TYPES: [&str; 7] = [
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/avif",
    "video/mp4",
    "video/webm",
];

/// A file name that sanitises to nothing is served under this one.
const FALLBACK_NAME: &str = "download";

/// A name is cut to this many UTF-8 bytes before it is percent-encoded.
const MAX_NAME_BYTES: usize = 255;

/// Sets the content headers of `disposition`. `Inline` with a type outside the allowlist is rendered as an
/// attachment, so a hostile type is never served under its own type or inline.
pub(super) fn apply(
    headers: &mut HeaderMap,
    disposition: Disposition,
    media_type: Option<&str>,
    suggested_name: &str,
) -> Result<(), InconsistentDownload> {
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    let inline = match disposition {
        Disposition::Inline => media_type.and_then(inline_type),
        Disposition::Attachment => None,
    };
    if let Some(inline) = inline {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(inline));
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("inline"),
        );
        return Ok(());
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::try_from(attachment(suggested_name))
            .map_err(InconsistentDownload::FileName)?,
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox"),
    );
    Ok(())
}

fn inline_type(media_type: &str) -> Option<&'static str> {
    INLINE_TYPES
        .into_iter()
        .find(|allowed| allowed.eq_ignore_ascii_case(media_type))
}

pub(super) fn attachment(suggested_name: &str) -> String {
    format!(
        "attachment; filename*=UTF-8''{}",
        percent_encode(&sanitise(suggested_name))
    )
}

/// Drops control and bidi characters, turns path separators into `_`, cuts at [`MAX_NAME_BYTES`], and
/// replaces an empty or dots-only result with [`FALLBACK_NAME`].
fn sanitise(name: &str) -> String {
    let mut clean = String::new();
    for ch in name.chars() {
        let ch = if matches!(ch, '/' | '\\') { '_' } else { ch };
        if ch.is_control() || is_bidi_control(ch) {
            continue;
        }
        if clean.len() + ch.len_utf8() > MAX_NAME_BYTES {
            break;
        }
        clean.push(ch);
    }
    if clean.trim().is_empty() || clean.chars().all(|ch| ch == '.') {
        return FALLBACK_NAME.to_owned();
    }
    clean
}

fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// RFC 8187 `attr-char` stays; every other byte of the UTF-8 name becomes `%XX`, which escapes quotes.
fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
