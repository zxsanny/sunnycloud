use rusqlite::{Connection, OptionalExtension, Row};

use super::super::{NewPair, Pair, PairState};
use super::entries::{conversion_failure, uuid_from_blob};

pub(in crate::local_index) fn insert(conn: &Connection, pair: &NewPair) -> rusqlite::Result<i64> {
    conn.prepare_cached(
        "INSERT INTO pair (server_node_id, local_root, rules, root_sentinel, case_insensitive, state) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'active')",
    )?
    .execute(rusqlite::params![
        pair.server_node_id.as_bytes().as_slice(),
        pair.local_root,
        pair.rules,
        pair.root_sentinel,
        pair.case_insensitive,
    ])?;
    Ok(conn.last_insert_rowid())
}

pub(in crate::local_index) fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<Pair>> {
    conn.prepare_cached(
        "SELECT id, server_node_id, local_root, rules, root_sentinel, case_insensitive, state, hold_detail \
         FROM pair WHERE id = ?1",
    )?
    .query_row([id], from_row)
    .optional()
}

pub(in crate::local_index) fn case_insensitive(
    conn: &Connection,
    id: i64,
) -> rusqlite::Result<Option<bool>> {
    conn.prepare_cached("SELECT case_insensitive FROM pair WHERE id = ?1")?
        .query_row([id], |row| row.get(0))
        .optional()
}

/// Returns whether the pair exists.
pub(in crate::local_index) fn set_state(
    conn: &Connection,
    id: i64,
    state: PairState,
    hold_detail: Option<&str>,
) -> rusqlite::Result<bool> {
    let changed = conn
        .prepare_cached("UPDATE pair SET state = ?2, hold_detail = ?3 WHERE id = ?1")?
        .execute(rusqlite::params![id, state.as_str(), hold_detail])?;
    Ok(changed == 1)
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Pair> {
    let state: String = row.get(6)?;
    Ok(Pair {
        id: row.get(0)?,
        server_node_id: uuid_from_blob(&row.get::<_, Vec<u8>>(1)?, 1)?,
        local_root: row.get(2)?,
        rules: row.get(3)?,
        root_sentinel: row.get(4)?,
        case_insensitive: row.get(5)?,
        state: PairState::parse(&state).ok_or_else(|| conversion_failure(6, "pair state"))?,
        hold_detail: row.get(7)?,
    })
}
