//! The JSON of `POST /thumbnail-states` and the fixed words of the media routes.

use std::collections::HashMap;

use contracts::{DerivedKind, ThumbState};
use sc_ids::FileHash;
use serde_json::{Map, Value, json};

use crate::registry::SchemaRef;

/// The most hashes one request may ask about (08 section 5).
pub(super) const MAX_HASHES: usize = 1000;

/// The type 08 produces for each kind (08 section 7: WebP stills, MP4 transcodes). `BlobRef` carries no type
/// and `DerivedOpener` returns none, so 11 names it for 03.
pub(in crate::registry) const fn media_type(kind: DerivedKind) -> &'static str {
    match kind {
        DerivedKind::Thumb | DerivedKind::Preview => "image/webp",
        DerivedKind::Transcode => "video/mp4",
    }
}

/// `size=small|preview`; absent is `small`, anything else is `None`.
pub(in crate::registry) fn thumbnail_kind(size: Option<&str>) -> Option<DerivedKind> {
    match size {
        None | Some("small") => Some(DerivedKind::Thumb),
        Some("preview") => Some(DerivedKind::Preview),
        Some(_) => None,
    }
}

/// `{content_hashes: [<64 hex digits>, ...]}`, with a count the caller checks. `None` for another shape.
pub(super) fn hash_texts(body: &Value) -> Option<Vec<&str>> {
    let [hashes] = crate::json_body::fields(body, ["content_hashes"])?;
    hashes.as_array()?.iter().map(Value::as_str).collect()
}

/// A content hash as 64 hex digits, upper or lower case.
pub(super) fn parse_hash(text: &str) -> Option<FileHash> {
    let digits = text.as_bytes();
    if digits.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (byte, pair) in bytes.iter_mut().zip(digits.chunks_exact(2)) {
        *byte = (hex(pair[0])? << 4) | hex(pair[1])?;
    }
    Some(FileHash::from_bytes(bytes))
}

fn hex(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// `{states: {<hash>: ready | pending | failed}}`. A hash with no state to report (no job planned, or unknown to
/// the account) is left out, so a hash of another account looks like one nobody has.
pub(super) fn states(found: &HashMap<FileHash, ThumbState>) -> Value {
    let mut states = Map::new();
    for (hash, state) in found {
        let word = match state {
            ThumbState::Ready => "ready",
            ThumbState::Pending => "pending",
            ThumbState::Failed => "failed",
            ThumbState::None => continue,
        };
        states.insert(hash.to_string(), json!(word));
    }
    json!({"states": states})
}

pub(super) const STATES_REQUEST: SchemaRef = SchemaRef {
    name: "ThumbnailStatesRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["content_hashes"],
            "additionalProperties": false,
            "properties": {
                "content_hashes": {
                    "type": "array",
                    "maxItems": MAX_HASHES,
                    "items": {"type": "string", "description": "64 hex digits."}
                }
            }
        })
    },
};

pub(super) const STATES_RESPONSE: SchemaRef = SchemaRef {
    name: "ThumbnailStates",
    build: || {
        json!({
            "type": "object",
            "required": ["states"],
            "properties": {
                "states": {
                    "type": "object",
                    "additionalProperties": {"type": "string", "enum": ["ready", "pending", "failed"]}
                }
            }
        })
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "00ff10aB00ff10aB00ff10aB00ff10aB00ff10aB00ff10aB00ff10aB00ff10aB";

    #[test]
    fn a_hash_is_parsed_from_sixty_four_hex_digits_of_either_case() {
        // Arrange
        let canonical = HASH.to_ascii_lowercase();

        // Act
        let parsed = parse_hash(HASH).expect("a hash");

        // Assert
        assert_eq!(parsed.to_string(), canonical);
    }

    #[test]
    fn a_text_that_is_not_sixty_four_hex_digits_is_not_a_hash() {
        // Arrange
        let cases = [
            "",
            "00ff",
            &HASH[..63],
            &format!("{HASH}0"),
            &HASH.replace('a', "g"),
            "é0ff10aB00ff10aB00ff10aB00ff10aB00ff10aB00ff10aB00ff10aB00ff10a",
        ];

        // Act
        let parsed: Vec<bool> = cases
            .iter()
            .map(|case| parse_hash(case).is_some())
            .collect();

        // Assert
        assert_eq!(parsed, [false; 6]);
    }

    #[test]
    fn the_size_word_picks_the_derived_kind_and_each_kind_has_its_type() {
        // Arrange
        let sizes = [
            None,
            Some("small"),
            Some("preview"),
            Some("large"),
            Some(""),
        ];

        // Act
        let kinds: Vec<Option<DerivedKind>> = sizes.into_iter().map(thumbnail_kind).collect();

        // Assert
        assert_eq!(
            kinds,
            [
                Some(DerivedKind::Thumb),
                Some(DerivedKind::Thumb),
                Some(DerivedKind::Preview),
                None,
                None
            ]
        );
        assert_eq!(media_type(DerivedKind::Preview), "image/webp");
        assert_eq!(media_type(DerivedKind::Transcode), "video/mp4");
    }

    #[test]
    fn the_answer_names_ready_pending_and_failed_and_leaves_out_the_rest() {
        // Arrange
        let hash = |byte| FileHash::from_bytes([byte; 32]);
        let found = HashMap::from([
            (hash(1), ThumbState::Ready),
            (hash(2), ThumbState::Pending),
            (hash(3), ThumbState::Failed),
            (hash(4), ThumbState::None),
        ]);

        // Act
        let answer = states(&found);

        // Assert
        assert_eq!(
            answer,
            json!({"states": {
                hash(1).to_string(): "ready",
                hash(2).to_string(): "pending",
                hash(3).to_string(): "failed",
            }})
        );
    }
}
