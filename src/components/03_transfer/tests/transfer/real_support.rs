//! The rig with the real counterparts of 03's account and media ports (AZ-3760, E-14/c15): the `AccountGuard` of
//! 03 and of 02's `publish` is 01's `IdentityAccounts`, and 08's `MediaEnqueueEffect` sits in 03's composition-root
//! effect slot after 03's own effects. The account is created as 01 creates one: its row, then the real
//! `initialize` of 07a, 05 and 02 in one system-scope transaction (`real_account_repo`); 03's own `initialize`
//! writes nothing and is not run. The move to `deleting` is the real `AccountAdmin::begin_deletion`. The reader,
//! writer, ledger and `publish` decorators of the rig delegate to the real 04, 07a and 02.

use std::sync::Arc;
use std::time::Duration;

use contracts::{AccountAdmin, AccountLifecycleHook};
use media_api::{JobMetrics, JobPlanner, JobQueue, MediaConfig, MediaEnqueueEffect};
use sc_clock::Clock;
use sc_config::Secret;
use sc_db::Scope;
use sc_ids::FileHash;
use sc_observability::{BuildInfo, Metrics};
use sunnycloud_blobstore::{FileEpochMarker, GcQueue};
use sunnycloud_identity::IdentityAccounts;
use sunnycloud_quota::{PgQuotaAdmin, QuotaConfig};
use sunnycloud_sync_feed::PgFeedLifecycle;
use sunnycloud_tree::PgTreeLifecycle;

use crate::real_account_repo::create_account;
use crate::rig_wiring::{Site, Wiring};
use crate::start_support::{GIB, Rig, close, transfer_config};
use crate::support::Fixture;

const DAY: Duration = Duration::from_secs(86_400);

/// A JPEG head followed by `seed`, so every seed is content of its own that 03 sniffs as `image/jpeg`.
pub fn jpeg(seed: &str) -> Vec<u8> {
    let mut data = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00".to_vec();
    data.extend_from_slice(seed.as_bytes());
    data
}

pub struct Real {
    pub rig: Rig,
    pub identity: Arc<IdentityAccounts>,
}

impl Real {
    pub async fn new() -> Real {
        let wire = async |site: Site| real_wiring(site).await;
        let (rig, identity) =
            Rig::wired(Fixture::new().await, transfer_config(), 0, false, 1, wire).await;
        Real { rig, identity }
    }

    /// The real `AccountAdmin::begin_deletion`, in the account's scope.
    pub async fn begin_deletion(&self) {
        let account = self.rig.account;
        self.rig
            .fixture
            .api
            .run(Scope::Account(account), async |tx| {
                self.identity.begin_deletion(tx, account).await
            })
            .await
            .expect("begin deletion");
    }

    /// Job rows of the account for content `hash`.
    pub async fn jobs_of(&self, hash: &FileHash) -> i64 {
        let hex: String = hash.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        self.rig
            .count(&format!(
                "SELECT count(*) FROM jobs WHERE account_id = '{}' AND content_hash = '\\x{hex}'",
                self.rig.account
            ))
            .await
    }

    /// Every job row of the account.
    pub async fn jobs(&self) -> i64 {
        self.rig
            .count(&format!(
                "SELECT count(*) FROM jobs WHERE account_id = '{}'",
                self.rig.account
            ))
            .await
    }

    pub async fn seq(&self) -> i64 {
        self.rig
            .count(&format!(
                "SELECT seq FROM account_feeds WHERE account_id = '{}'",
                self.rig.account
            ))
            .await
    }

    /// Every row of the account a transfer entry point or a commit can touch, as text.
    pub async fn rows(&self) -> String {
        let mut admin = self.rig.fixture.db.admin().await;
        let text = sqlx::query_scalar(
            "SELECT concat_ws(' | ', \
             (SELECT string_agg(n::text, ',' ORDER BY n.id) FROM nodes n WHERE n.account_id = $1), \
             (SELECT string_agg(r::text, ',' ORDER BY r.id) FROM revisions r WHERE r.account_id = $1), \
             (SELECT string_agg(b::text, ',' ORDER BY b.id) FROM blobs b WHERE b.account_id = $1), \
             (SELECT string_agg(s::text, ',' ORDER BY s.id) FROM upload_sessions s WHERE s.account_id = $1), \
             (SELECT string_agg(j::text, ',' ORDER BY j.id) FROM jobs j WHERE j.account_id = $1), \
             (SELECT string_agg(c::text, ',' ORDER BY c.commit_id) FROM commit_receipts c \
              WHERE c.account_id = $1), \
             (SELECT q::text FROM quota q WHERE q.account_id = $1), \
             (SELECT f::text FROM account_feeds f WHERE f.account_id = $1))",
        )
        .bind(self.rig.account)
        .fetch_one(&mut admin)
        .await
        .expect("rows of the account");
        close(admin).await;
        text
    }

    pub async fn finish(self) {
        let Real { rig, identity } = self;
        drop(identity);
        rig.finish().await;
    }
}

/// The 08 settings the planner and queue read; job planning uses none of the durations.
fn media_config() -> MediaConfig {
    MediaConfig {
        job_lease: Duration::from_secs(60),
        job_attempts: 3,
        job_retry_backoff: Duration::from_secs(1),
        bulk_queue_threshold: 200,
        job_terminal_ttl: DAY,
        job_reap_interval: Duration::from_secs(60),
        derived_janitor_interval: Duration::from_secs(3600),
        worker_status_interval: Duration::from_secs(30),
        derived_max_thumb: 1 << 20,
        derived_max_preview: 1 << 20,
        derived_max_transcode: 1 << 20,
        worker_api_token: Secret::new("worker-token".to_owned()),
        worker_api_token_next: None,
    }
}

/// 01's guard, an account made by the real hooks, and 08's media effect for it in the slot.
async fn real_wiring(site: Site) -> (Wiring, Arc<IdentityAccounts>) {
    let Site {
        api,
        mut owner,
        blob_config,
        clock,
    } = site;
    let clock: Arc<dyn Clock> = clock;
    let identity = Arc::new(IdentityAccounts::new(api.clone(), clock.clone()));
    let gc = GcQueue::new(&blob_config, api.clone(), clock.clone()).expect("gc queue");
    let marker = FileEpochMarker::new(&blob_config).expect("epoch marker");
    let quota = QuotaConfig {
        default_quota_bytes: 10 * GIB,
    };
    let hooks: Vec<Arc<dyn AccountLifecycleHook>> = vec![
        Arc::new(PgQuotaAdmin::new(api.clone(), quota)),
        Arc::new(PgFeedLifecycle::new(
            api.clone(),
            clock.clone(),
            Arc::new(marker),
        )),
        Arc::new(PgTreeLifecycle::new(
            api.clone(),
            Arc::new(gc),
            clock.clone(),
        )),
    ];
    let (account, root) = create_account(&api, &mut owner, &hooks).await;
    close(owner).await;
    let media = media_config();
    let registry = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics");
    let metrics = Arc::new(JobMetrics::register(&registry).expect("job metrics"));
    let queue = Arc::new(JobQueue::new(api, clock, &media, metrics));
    let effect = MediaEnqueueEffect::new(queue, JobPlanner::from_config(&media), account);
    let wiring = Wiring {
        guard: identity.clone(),
        effects: vec![Arc::new(effect)],
        account,
        root,
    };
    (wiring, identity)
}
