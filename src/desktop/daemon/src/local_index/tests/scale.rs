//! AC-2: 100,000 entries in two pairs; each lookup uses its index; file size and resident memory are recorded
//! against the 14 §4 measurement (about 11 MB file, 7 MB resident). The size assertion is a regression guard, not the
//! documented figure: with 64 bytes of ids and hash per row, a 50-character path and two secondary indexes that each
//! repeat the primary key, a row costs about 320 bytes.

use sync_core_proto::Cursor;
use uuid::Uuid;

use super::{entry, new_pair, open};
use crate::local_index::repo::entries;
use crate::local_index::{Entry, EntryState, INDEX_FILE_NAME};
use crate::platform_fs::FileId;

const PER_PAIR: u64 = 50_000;

fn resident_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn plan(
    store: &crate::local_index::LocalIndexStore,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Vec<String> {
    store
        .read(|conn| {
            let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            let rows = stmt
                .query_map(params, |row| row.get::<_, String>(3))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .expect("plan")
}

#[test]
fn each_lookup_of_a_100k_entry_index_uses_its_index_and_the_file_stays_near_the_documented_size() {
    // Arrange
    let fixture = open();
    let store = &fixture.store;
    let rss_before = resident_kib();
    let pairs = [
        store.add_pair(&new_pair(false)).expect("pair"),
        store.add_pair(&new_pair(false)).expect("pair"),
    ];
    for chunk in 0..(PER_PAIR / 1000) {
        let mut batch: Vec<Entry> = Vec::new();
        for (slot, pair) in pairs.iter().enumerate() {
            for n in chunk * 1000..(chunk + 1) * 1000 {
                let id = n + slot as u64 * PER_PAIR;
                let mut e = entry(
                    *pair,
                    &format!(
                        "projects/area-{}/folder-{}/document-{n}.txt",
                        n % 20,
                        n % 400
                    ),
                    id,
                );
                if n % 100 == 0 {
                    e.state = EntryState::Dirty;
                }
                batch.push(e);
            }
        }
        store.upsert_entries(&batch).expect("upsert");
    }
    store.set_cursor(Cursor::new(1, 1)).expect("cursor");
    store
        .write(|conn| Ok(conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?))
        .expect("checkpoint");

    // Act
    let probe = 31_337;
    let by_path = store
        .get_entry(
            pairs[1],
            &format!(
                "projects/area-{}/folder-{}/document-{probe}.txt",
                probe % 20,
                probe % 400
            ),
        )
        .expect("by path");
    let by_file = store
        .find_by_file_id(
            pairs[0],
            FileId {
                volume: 1,
                index: probe,
            },
        )
        .expect("by file id");
    let by_node = store
        .find_by_node(pairs[1], Uuid::from_u128(u128::from(probe + PER_PAIR) + 1))
        .expect("by node");
    let dirty = store
        .entries_in_state(pairs[0], EntryState::Dirty)
        .expect("dirty");
    let (path, file_id, node, state) = (
        "projects/a".to_owned(),
        "1:2".to_owned(),
        Uuid::from_u128(5).as_bytes().to_vec(),
        "dirty",
    );
    let plans = [
        ("path", plan(store, entries::BY_PATH, &[&pairs[0], &path])),
        (
            "file id",
            plan(store, entries::BY_FILE_ID, &[&pairs[0], &file_id]),
        ),
        ("node", plan(store, entries::BY_NODE, &[&pairs[0], &node])),
        (
            "not synced",
            plan(store, entries::NOT_SYNCED, &[&pairs[0], &state]),
        ),
    ];
    let file_bytes = std::fs::metadata(fixture.dir.path().join(INDEX_FILE_NAME))
        .expect("meta")
        .len();
    let rss_after = resident_kib();

    // Assert
    assert_eq!(by_path.expect("by path").size, (probe + PER_PAIR) * 10);
    assert_eq!(by_file.len(), 1);
    assert_eq!(by_node.len(), 1);
    assert_eq!(dirty.len(), (PER_PAIR / 100) as usize);
    let expected = [
        ("path", "PRIMARY KEY"),
        ("file id", "USING INDEX entry_file_id"),
        ("node", "USING INDEX entry_node_id"),
        ("not synced", "USING INDEX entry_not_synced"),
    ];
    for ((name, lines), (_, needle)) in plans.iter().zip(expected) {
        assert!(
            lines.len() == 1 && lines[0].starts_with("SEARCH") && lines[0].contains(needle),
            "{name}: {lines:?}"
        );
    }
    eprintln!(
        "AC-2 measurement: {} entries, index file {:.1} MB (14 §4: about 11 MB), resident growth {} (14 §4: about 7 MB, whole daemon)",
        2 * PER_PAIR,
        file_bytes as f64 / 1_000_000.0,
        match (rss_before, rss_after) {
            (Some(a), Some(b)) => format!("{:.1} MB", b.saturating_sub(a) as f64 / 1000.0),
            _ => "not measurable here".to_owned(),
        }
    );
    assert!(file_bytes < 40_000_000, "{file_bytes} bytes");
}
