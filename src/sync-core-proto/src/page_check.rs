use std::fmt;

use thiserror::Error;

use crate::cursor::Cursor;
use crate::feed::ChangePage;

/// The feed-page rule a page broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageRule {
    /// Items are not ascending by `(seq, node_id)`.
    P1,
    /// An item has `seq <= cursor.seq`.
    P2,
    /// An item has `seq > next_cursor.seq`.
    P3,
    /// `next_cursor.epoch != cursor.epoch`.
    P4,
    /// `next_cursor < cursor`.
    P5,
    /// Two items share `(seq, node_id)`.
    P6,
}

impl fmt::Display for PageRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("feed page violates {rule}: {detail}")]
pub struct PageViolation {
    pub rule: PageRule,
    pub detail: String,
}

fn violation(rule: PageRule, detail: String) -> PageViolation {
    PageViolation { rule, detail }
}

/// Checks a page against the cursor that asked for it (AC-69); a page is never repaired.
///
/// Order: the cursors first (P4, P5), then the items in page order (P2, P3, then against the previous item
/// P6 for an equal `(seq, node_id)` and P1 for a smaller one). The first violation found is returned.
pub fn check_page(cursor: Cursor, page: &ChangePage) -> Result<(), PageViolation> {
    let next = page.next_cursor;
    if next.epoch != cursor.epoch {
        return Err(violation(
            PageRule::P4,
            format!("next cursor {next} is in another epoch than {cursor}"),
        ));
    }
    if next < cursor {
        return Err(violation(
            PageRule::P5,
            format!("next cursor {next} is before {cursor}"),
        ));
    }
    let mut previous: Option<(u64, uuid::Uuid)> = None;
    for item in &page.items {
        if item.seq <= cursor.seq {
            return Err(violation(
                PageRule::P2,
                format!(
                    "item {} has seq {} <= {}",
                    item.node_id, item.seq, cursor.seq
                ),
            ));
        }
        if item.seq > next.seq {
            return Err(violation(
                PageRule::P3,
                format!("item {} has seq {} > {}", item.node_id, item.seq, next.seq),
            ));
        }
        let key = (item.seq, item.node_id);
        if let Some(before) = previous {
            if key == before {
                return Err(violation(
                    PageRule::P6,
                    format!("duplicate item ({}, {})", item.seq, item.node_id),
                ));
            }
            if key < before {
                return Err(violation(
                    PageRule::P1,
                    format!(
                        "item ({}, {}) follows ({}, {})",
                        item.seq, item.node_id, before.0, before.1
                    ),
                ));
            }
        }
        previous = Some(key);
    }
    Ok(())
}
