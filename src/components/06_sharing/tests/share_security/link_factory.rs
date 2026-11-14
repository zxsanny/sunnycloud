//! The link factory of 06 `tests.md`: seeded accounts with live files (rows written as the schema owner, with a
//! size and a media type on the current revision), then N links made by the real `create` of the service under
//! test, each on a random file of a random account, with a random lifetime (1 hour to 365 days) and, for a share
//! of them, a password from the corpus. A few creates run at once on the caller's runtime.

use std::sync::Arc;
use std::time::Duration;

use contracts::{CreatedShare, ShareService};
use sqlx::Connection;
use sunnycloud_sharing::PgShareService;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::corpora::{Seeded, passwords};
use crate::harness::Harness;
use crate::owner_support::account_with_root;
use crate::visitor_support::named_file;

const HOUR_SECS: u64 = 3600;
const MAX_TTL_SECS: u64 = 365 * 24 * HOUR_SECS;
/// The media type of every factory file's current revision.
pub const MEDIA_TYPE: &str = "image/jpeg";
/// Creates in flight at once: the test pool has five connections.
const IN_FLIGHT: usize = 8;

/// A live file of the factory, with what a visitor sees of it.
#[derive(Debug, Clone)]
pub struct FactoryFile {
    pub account: Uuid,
    pub node: Uuid,
    pub revision: Uuid,
    pub name: String,
    pub size: i64,
}

/// One link the factory made, with its file and the password it was given.
pub struct FactoryLink {
    pub file: FactoryFile,
    pub created: CreatedShare,
    pub password: Option<&'static str>,
}

pub struct LinkFactory {
    files: Vec<FactoryFile>,
    rng: Seeded,
}

impl LinkFactory {
    /// `accounts` accounts with `files_per_account` live files each.
    pub async fn new(
        harness: &Harness,
        accounts: usize,
        files_per_account: usize,
        seed: u64,
    ) -> LinkFactory {
        let mut conn = harness.fixture_conn().await;
        let mut files = Vec::new();
        for _ in 0..accounts {
            let account = Uuid::new_v4();
            let root = account_with_root(&mut conn, account).await;
            for n in 0..files_per_account {
                let name = format!("factory-file-{n}.jpg");
                let size = 1000 + i64::try_from(n).expect("small index");
                let node = named_file(&mut conn, account, root, &name, size, MEDIA_TYPE).await;
                let revision =
                    sqlx::query_scalar("SELECT current_revision_id FROM nodes WHERE id = $1")
                        .bind(node)
                        .fetch_one(&mut conn)
                        .await
                        .expect("read the current revision");
                files.push(FactoryFile {
                    account,
                    node,
                    revision,
                    name,
                    size,
                });
            }
        }
        conn.close().await.expect("close fixture connection");
        LinkFactory {
            files,
            rng: Seeded::new(seed),
        }
    }

    /// `count` links through `shares`; `password_percent` of them (0 to 100) get a corpus password.
    pub async fn links(
        &mut self,
        shares: &Arc<PgShareService>,
        count: usize,
        password_percent: usize,
    ) -> Vec<FactoryLink> {
        let corpus = passwords();
        let plans: Vec<(FactoryFile, Duration, Option<&'static str>)> = (0..count)
            .map(|_| {
                let file = self.files[self.rng.below(self.files.len())].clone();
                let span = usize::try_from(MAX_TTL_SECS - HOUR_SECS).expect("fits usize");
                let ttl = HOUR_SECS + u64::try_from(self.rng.below(span)).expect("fits u64");
                let password = (self.rng.below(100) < password_percent)
                    .then(|| corpus[self.rng.below(corpus.len())]);
                (file, Duration::from_secs(ttl), password)
            })
            .collect();
        let mut made: Vec<Option<FactoryLink>> = (0..count).map(|_| None).collect();
        let mut running = JoinSet::new();
        for (index, (file, ttl, password)) in plans.into_iter().enumerate() {
            if running.len() == IN_FLIGHT {
                let (at, link) = running
                    .join_next()
                    .await
                    .expect("a create")
                    .expect("joined");
                made[at] = Some(link);
            }
            let shares = Arc::clone(shares);
            running.spawn(async move {
                let created = shares
                    .create(file.account, file.node, Some(ttl), password)
                    .await
                    .expect("factory create");
                (
                    index,
                    FactoryLink {
                        file,
                        created,
                        password,
                    },
                )
            });
        }
        while let Some(joined) = running.join_next().await {
            let (at, link) = joined.expect("joined");
            made[at] = Some(link);
        }
        made.into_iter()
            .map(|link| link.expect("every create ran"))
            .collect()
    }
}
