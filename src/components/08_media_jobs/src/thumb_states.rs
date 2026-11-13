//! `ThumbnailStateSource` (E-18/c09, `derived_access.md`): the thumbnail state of many contents in one short
//! account-scoped transaction of two batched index probes, one on `derived` and one on `jobs`.

use std::collections::{HashMap, HashSet};

use contracts::{ThumbState, ThumbnailStateError, ThumbnailStateSource, async_trait};
use sc_db::{Db, DbError, Scope};
use sc_ids::FileHash;
use uuid::Uuid;

use crate::derived_access_repo::{self, ThumbJob};
use crate::planner::JobPlanner;

pub struct ThumbnailStates {
    db: Db,
    params_version: u32,
}

impl ThumbnailStates {
    /// States are read at the params version the planner enqueues with.
    pub fn new(db: Db, planner: JobPlanner) -> Self {
        ThumbnailStates {
            db,
            params_version: planner.params_version(),
        }
    }
}

#[async_trait]
impl ThumbnailStateSource for ThumbnailStates {
    /// Another account's hash is invisible to both probes (the account predicate and row-level security), so it
    /// is `None` like an unknown one. The 1,000-hash cap of the route is 11's.
    async fn states(
        &self,
        account: Uuid,
        hashes: &[FileHash],
    ) -> Result<HashMap<FileHash, ThumbState>, ThumbnailStateError> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let params_version = self.params_version;
        let (ready, jobs) = self
            .db
            .run(Scope::Account(account), async |tx| {
                let ready =
                    derived_access_repo::thumb_ready(tx, account, hashes, params_version).await?;
                let jobs =
                    derived_access_repo::thumb_jobs(tx, account, hashes, params_version).await?;
                Ok::<_, DbError>((ready, jobs))
            })
            .await
            .map_err(|error| ThumbnailStateError::Unexpected(Box::new(error)))?;
        Ok(merge(hashes, &ready, &jobs))
    }
}

/// One state per asked hash: a `thumb` row is `Ready`; else a queued or running job is `Pending`; else a dead
/// job is `Failed`; else `None`.
pub(crate) fn merge(
    hashes: &[FileHash],
    ready: &[FileHash],
    jobs: &[(FileHash, ThumbJob)],
) -> HashMap<FileHash, ThumbState> {
    let ready: HashSet<&FileHash> = ready.iter().collect();
    let mut states: HashMap<FileHash, ThumbState> = hashes
        .iter()
        .map(|hash| {
            let state = if ready.contains(hash) {
                ThumbState::Ready
            } else {
                ThumbState::None
            };
            (*hash, state)
        })
        .collect();
    for (hash, job) in jobs {
        let Some(state) = states.get_mut(hash) else {
            continue;
        };
        *state = match (*state, job) {
            (ThumbState::Ready, _) => ThumbState::Ready,
            (_, ThumbJob::Pending) => ThumbState::Pending,
            (ThumbState::Pending, ThumbJob::Dead) => ThumbState::Pending,
            (_, ThumbJob::Dead) => ThumbState::Failed,
        };
    }
    states
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(n: u8) -> FileHash {
        FileHash::from_bytes([n; 32])
    }

    #[test]
    fn the_merge_gives_ready_pending_failed_and_none_as_listed() {
        // Arrange
        let hashes = [hash(1), hash(2), hash(3), hash(4), hash(5), hash(6)];
        let ready = [hash(1), hash(6)];
        let jobs = [
            (hash(2), ThumbJob::Pending),
            (hash(3), ThumbJob::Dead),
            (hash(5), ThumbJob::Dead),
            (hash(5), ThumbJob::Pending),
            (hash(6), ThumbJob::Dead),
            (hash(9), ThumbJob::Pending),
        ];

        // Act
        let states = merge(&hashes, &ready, &jobs);

        // Assert
        let expected = HashMap::from([
            (hash(1), ThumbState::Ready),
            (hash(2), ThumbState::Pending),
            (hash(3), ThumbState::Failed),
            (hash(4), ThumbState::None),
            (hash(5), ThumbState::Pending),
            (hash(6), ThumbState::Ready),
        ]);
        assert_eq!(states, expected);
    }

    #[test]
    fn no_hash_gives_an_empty_map() {
        // Arrange
        let hashes: [FileHash; 0] = [];

        // Act
        let states = merge(&hashes, &[], &[]);

        // Assert
        assert!(states.is_empty());
    }
}
