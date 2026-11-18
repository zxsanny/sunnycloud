//! A second connection to the checkpoint file: reads what the run stored, and installs failures into SQLite.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Db(PathBuf);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetRow {
    pub state: String,
    pub content_hash: Option<Vec<u8>>,
    pub client_key: Option<Uuid>,
    pub node_id: Option<Vec<u8>>,
    pub revision_id: Option<Vec<u8>>,
    pub upload_name: Option<String>,
}

impl Db {
    pub fn new(path: &Path) -> Self {
        Self(path.to_owned())
    }

    fn connect(&self) -> Connection {
        let connection = Connection::open(&self.0).expect("open the checkpoint file");
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .expect("busy timeout");
        connection
    }

    pub fn exec(&self, sql: &str) {
        self.connect().execute_batch(sql).expect("execute sql");
    }

    pub fn states(&self) -> BTreeMap<i64, String> {
        let connection = self.connect();
        let mut statement = connection
            .prepare("SELECT asset_id, state FROM asset")
            .expect("prepare");
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows")
    }

    pub fn count(&self, state: &str) -> usize {
        self.states()
            .values()
            .filter(|found| *found == state)
            .count()
    }

    pub fn asset(&self, asset_id: i64) -> Option<AssetRow> {
        self.connect()
            .query_row(
                "SELECT state, content_hash, client_key, node_id, revision_id, upload_name FROM asset WHERE asset_id = ?1",
                [asset_id],
                |row| {
                    Ok(AssetRow {
                        state: row.get(0)?,
                        content_hash: row.get(1)?,
                        client_key: row
                            .get::<_, Option<Vec<u8>>>(2)?
                            .map(|bytes| Uuid::from_slice(&bytes).expect("a key")),
                        node_id: row.get(3)?,
                        revision_id: row.get(4)?,
                        upload_name: row.get(5)?,
                    })
                },
            )
            .optional()
            .expect("asset row")
    }

    pub fn asset_of_key(&self, key: Uuid) -> Option<AssetRow> {
        let id: Option<i64> = self
            .connect()
            .query_row(
                "SELECT asset_id FROM asset WHERE client_key = ?1",
                [key.as_bytes().to_vec()],
                |row| row.get(0),
            )
            .optional()
            .expect("asset of key");
        id.and_then(|id| self.asset(id))
    }

    pub fn keys(&self) -> BTreeMap<i64, Uuid> {
        let connection = self.connect();
        let mut statement = connection
            .prepare("SELECT asset_id, client_key FROM asset WHERE client_key IS NOT NULL")
            .expect("prepare");
        statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    Uuid::from_slice(&row.get::<_, Vec<u8>>(1)?).expect("a key"),
                ))
            })
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows")
    }

    pub fn cursor_text(&self, key: &str) -> Option<String> {
        self.connect()
            .query_row("SELECT value FROM cursor WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .expect("cursor text")
    }

    pub fn cursor_bytes(&self, key: &str) -> Option<Vec<u8>> {
        self.connect()
            .query_row("SELECT value FROM cursor WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .expect("cursor bytes")
    }

    pub fn batch_ids(&self) -> Vec<Uuid> {
        let connection = self.connect();
        let mut statement = connection
            .prepare("SELECT commit_id FROM batch")
            .expect("prepare");
        statement
            .query_map([], |row| {
                Ok(Uuid::from_slice(&row.get::<_, Vec<u8>>(0)?).expect("an id"))
            })
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows")
    }

    pub fn session_count(&self) -> usize {
        let count: i64 = self
            .connect()
            .query_row("SELECT count(*) FROM session", [], |row| row.get(0))
            .expect("session count");
        usize::try_from(count).expect("a count")
    }

    /// Every later write to `asset` fails, as a full disk would.
    pub fn fail_asset_writes(&self) {
        self.exec(
            "CREATE TRIGGER fail_asset_writes BEFORE UPDATE ON asset BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        );
    }

    pub fn heal_asset_writes(&self) {
        self.exec("DROP TRIGGER fail_asset_writes;");
    }

    pub fn fail_token_writes(&self) {
        self.exec(
            "CREATE TRIGGER fail_token BEFORE INSERT ON cursor WHEN NEW.key = 'change_token' \
             BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        );
    }

    pub fn heal_token_writes(&self) {
        self.exec("DROP TRIGGER fail_token;");
    }
}
