use serde::{Deserialize, Serialize};

/// Server node id: a UUID in its canonical 8-4-4-4-12 hexadecimal form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NodeId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("node id is not a canonical UUID string")]
pub struct InvalidNodeId;

impl NodeId {
    pub fn parse(text: &str) -> Result<Self, InvalidNodeId> {
        if is_canonical_uuid(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidNodeId)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NodeId {
    type Error = InvalidNodeId;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        if is_canonical_uuid(&text) {
            Ok(Self(text))
        } else {
            Err(InvalidNodeId)
        }
    }
}

impl From<NodeId> for String {
    fn from(id: NodeId) -> Self {
        id.0
    }
}

fn is_canonical_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// Local pair id (`pair.id INTEGER` in the LocalIndex).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PairId(pub i64);

/// Opaque cursor of a paged listing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageToken(pub String);
