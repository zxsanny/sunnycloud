use uuid::Uuid;

use crate::error::Rejection;

/// Size limit of `detail` as the database measures it (`octet_length(detail::text)`).
pub const MAX_DETAIL_BYTES: usize = 2048;

const MAX_TOKEN_BYTES: usize = 64;

/// A key, enumeration code or subject type: 1 to 64 bytes of `a-z`, `0-9` and `_`. It contains no space,
/// separator, `@` or `/`, so a name, an address or a path is not a token. A uuid has hyphens, so a code never
/// reads back as an id.
pub(crate) fn is_token(text: &str) -> bool {
    (1..=MAX_TOKEN_BYTES).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// One value of an [`AuditDetail`]: an id, an enumeration code or an integer. There is no text variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailValue {
    Id(Uuid),
    /// The `&'static str` of an enumeration variant. A runtime `String` does not fit.
    Code(&'static str),
    Int(i64),
}

/// Named typed values of one event. Keys and codes are `&'static str`, so a value read from a request, a
/// file name or an email cannot be passed (the compile-fail examples below). At most [`MAX_DETAIL_BYTES`] bytes
/// once serialised to `jsonb`.
///
/// ```compile_fail,E0308
/// let name = String::from("alice@example.org");
/// let _detail = sc_audit::AuditDetail::new().with_code("name", name);
/// ```
///
/// ```compile_fail,E0597
/// let name = String::from("alice@example.org");
/// let _detail = sc_audit::AuditDetail::new().with_code("name", &name);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditDetail {
    entries: Vec<(&'static str, DetailValue)>,
}

impl AuditDetail {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_id(mut self, key: &'static str, value: Uuid) -> Self {
        self.entries.push((key, DetailValue::Id(value)));
        self
    }

    pub fn with_code(mut self, key: &'static str, code: &'static str) -> Self {
        self.entries.push((key, DetailValue::Code(code)));
        self
    }

    pub fn with_int(mut self, key: &'static str, value: i64) -> Self {
        self.entries.push((key, DetailValue::Int(value)));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[(&'static str, DetailValue)] {
        &self.entries
    }

    /// The `jsonb` text of the detail, in the form PostgreSQL prints it (`{"a": 1, "b": "x"}`), so the length
    /// checked here is the length the database `CHECK` measures. `None` for an empty detail (stored as NULL).
    pub(crate) fn render(&self) -> Result<Option<String>, Rejection> {
        if self.entries.is_empty() {
            return Ok(None);
        }
        let mut text = String::from("{");
        for (index, (key, value)) in self.entries.iter().enumerate() {
            if !is_token(key) {
                return Err(Rejection::InvalidToken {
                    place: "detail key",
                });
            }
            if self.entries[..index].iter().any(|(seen, _)| seen == key) {
                return Err(Rejection::DuplicateKey);
            }
            if index > 0 {
                text.push_str(", ");
            }
            text.push('"');
            text.push_str(key);
            text.push_str("\": ");
            match value {
                DetailValue::Id(id) => {
                    text.push('"');
                    text.push_str(&id.hyphenated().to_string());
                    text.push('"');
                }
                DetailValue::Code(code) => {
                    if !is_token(code) {
                        return Err(Rejection::InvalidToken {
                            place: "detail code",
                        });
                    }
                    text.push('"');
                    text.push_str(code);
                    text.push('"');
                }
                DetailValue::Int(number) => text.push_str(&number.to_string()),
            }
        }
        text.push('}');
        if text.len() > MAX_DETAIL_BYTES {
            return Err(Rejection::DetailTooLarge { bytes: text.len() });
        }
        Ok(Some(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_detail_is_null() {
        assert_eq!(AuditDetail::new().render(), Ok(None));
    }

    #[test]
    fn renders_in_the_jsonb_text_form() {
        let id = Uuid::from_u128(1);
        let detail = AuditDetail::new()
            .with_id("link_id", id)
            .with_code("limiter", "share_password")
            .with_int("attempts", -3);

        let text = detail.render().expect("valid detail");

        assert_eq!(
            text.as_deref(),
            Some(
                "{\"link_id\": \"00000000-0000-0000-0000-000000000001\", \
                 \"limiter\": \"share_password\", \"attempts\": -3}"
            )
        );
    }

    #[test]
    fn a_key_or_code_that_is_not_a_token_is_invalid() {
        for bad in ["", "Name", "a b", "a@b", "a/b", "a-b", "é", &"x".repeat(65)] {
            let key = AuditDetail::new().with_int(leak(bad), 1).render();
            let code = AuditDetail::new().with_code("k", leak(bad)).render();

            assert!(matches!(key, Err(Rejection::InvalidToken { .. })), "{bad}");
            assert!(matches!(code, Err(Rejection::InvalidToken { .. })), "{bad}");
        }
    }

    #[test]
    fn a_repeated_key_is_rejected() {
        let detail = AuditDetail::new().with_int("n", 1).with_int("n", 2);

        assert_eq!(detail.render(), Err(Rejection::DuplicateKey));
    }

    #[test]
    fn the_limit_is_2048_bytes_of_rendered_text() {
        let at_limit = sized_detail(MAX_DETAIL_BYTES);
        let over = sized_detail(MAX_DETAIL_BYTES + 1);

        let ok = at_limit.render().expect("2048 bytes fit");
        let err = over.render();

        assert_eq!(ok.map(|text| text.len()), Some(MAX_DETAIL_BYTES));
        assert_eq!(
            err,
            Err(Rejection::DetailTooLarge {
                bytes: MAX_DETAIL_BYTES + 1
            })
        );
    }

    /// Integer entries whose rendered text is exactly `target` bytes: an entry with a key of `n` bytes adds
    /// `n + 7` (`, "` + key + `": 1`), and the braces replace the first separator.
    fn sized_detail(target: usize) -> AuditDetail {
        let mut lengths = Vec::new();
        let mut remaining = target;
        while remaining > 71 + 8 {
            lengths.push(64);
            remaining -= 71;
        }
        if remaining > 71 {
            lengths.push(30);
            remaining -= 37;
        }
        lengths.push(remaining - 7);
        let mut detail = AuditDetail::new();
        for (index, length) in lengths.into_iter().enumerate() {
            let key = format!("{index:03}{}", "k".repeat(length - 3));
            detail = detail.with_int(leak(&key), 1);
        }
        detail
    }

    fn leak(text: &str) -> &'static str {
        Box::leak(text.to_owned().into_boxed_str())
    }
}
