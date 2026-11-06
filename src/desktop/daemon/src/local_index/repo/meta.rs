use rusqlite::{Connection, OptionalExtension};

pub(in crate::local_index) fn get(
    conn: &Connection,
    key: &str,
) -> rusqlite::Result<Option<String>> {
    conn.prepare_cached("SELECT value FROM meta WHERE key = ?1")?
        .query_row([key], |row| row.get(0))
        .optional()
}

pub(in crate::local_index) fn set(
    conn: &Connection,
    key: &str,
    value: &str,
) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )?
    .execute([key, value])?;
    Ok(())
}
