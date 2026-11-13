//! Account creation on the harness: the sign-in world with the real lifecycle hooks of 02 (tree root, over 04's
//! GC queue), 05 (account feed, over 04's epoch marker in a scratch blob root) and 07a (quota row), a
//! fault-injecting hook double, open invite fixtures, and counts of every row account creation writes.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use contracts::{
    AccountLifecycleHook, EraseProgress, LifecycleError, SignInError, SignInOutcome, async_trait,
};
use sc_clock::Clock;
use sc_db::{Db, DbTx};
use sc_ids::{RawToken, TokenKind};
use sunnycloud_blobstore::{BlobstoreConfig, FileEpochMarker, GcQueue};
use sunnycloud_identity::{IdentityConfig, IdentitySignIn};
use sunnycloud_quota::{PgQuotaAdmin, QuotaConfig};
use sunnycloud_sync_feed::PgFeedLifecycle;
use sunnycloud_tree::PgTreeLifecycle;
use tempfile::TempDir;
use uuid::Uuid;

use super::provider::Grant;
use super::sign_in::{CLIENT_IP, HOST, World, begin_request, params, roomy_limiter, world_with};
use super::{Harness, START, load_settings};

/// An invite token of the fixtures; its stored hash is `sha256` of this text, as `begin` stores it.
pub const INVITE: &str = "invite-token-1";
/// One day in microseconds.
pub const DAY_MICROS: i64 = 86_400_000_000;

/// Rows of every table account creation writes, and the redeemed invites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rows {
    pub accounts: i64,
    pub identities: i64,
    pub root_nodes: i64,
    pub feeds: i64,
    pub quotas: i64,
    pub redeemed_invites: i64,
}

/// The sign-in world with a scratch blob root for the hooks; `pool_max` sizes the component's pool.
pub struct Rig {
    pub world: World,
    blob_root: TempDir,
}

impl Rig {
    /// The world without registered hooks.
    pub async fn unregistered(pool_max: u32, change: impl FnOnce(&mut IdentityConfig)) -> Rig {
        let mut world = world_with(change).await;
        let mut config = world.harness.db.config();
        config.db_pool_max = pool_max;
        world.harness.api = Db::connect(&config, sc_db::DbRole::Api)
            .await
            .expect("connect as the API role");
        let mut rig = Rig {
            world,
            blob_root: tempfile::tempdir().expect("scratch blob root"),
        };
        rig.world.service = rig.new_service();
        rig
    }

    /// The world with the real hooks of 02, 05 and 07a registered, then `extra`.
    pub async fn new(
        pool_max: u32,
        change: impl FnOnce(&mut IdentityConfig),
        extra: Vec<Arc<dyn AccountLifecycleHook>>,
    ) -> Rig {
        let rig = Rig::unregistered(pool_max, change).await;
        let mut hooks = rig.real_hooks();
        hooks.extend(extra);
        rig.world
            .service
            .register_hooks(hooks)
            .expect("first registration");
        rig
    }

    /// Another sign-in service on the component's pool, without hooks; the caller registers them.
    pub fn new_service(&self) -> IdentitySignIn {
        IdentitySignIn::new(
            self.world.harness.api.clone(),
            self.world.harness.clock.clone(),
            &self.world.harness.config,
            self.world.harness.audit(),
            roomy_limiter(&self.world.harness),
        )
        .expect("sign-in service")
    }

    /// The hooks the composition root registers, built the way it builds them, on the component's pool.
    pub fn real_hooks(&self) -> Vec<Arc<dyn AccountLifecycleHook>> {
        real_hooks(&self.world.harness, self.blob_root.path())
    }

    /// An open invite granting `role` (`user` or `admin`) that expires at `expires_at` (microseconds).
    pub async fn invite(&mut self, token: &str, role: &str, expires_at: i64) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO invites (id, kind, token_hash, grants_role, created_at, expires_at) \
             VALUES (gen_random_uuid(), 'invite', sha256(convert_to($1, 'UTF8')), $2, \
             'epoch'::timestamptz + $3::bigint * interval '1 microsecond', \
             'epoch'::timestamptz + $4::bigint * interval '1 microsecond') RETURNING id",
        )
        .bind(token)
        .bind(role)
        .bind(START.as_unix_micros())
        .bind(expires_at)
        .fetch_one(&mut self.world.conn)
        .await
        .expect("insert invite")
    }

    /// A member invite valid for a day.
    pub async fn member_invite(&mut self, token: &str) -> Uuid {
        self.invite(token, "user", START.as_unix_micros() + DAY_MICROS)
            .await
    }

    pub async fn rows(&mut self) -> Rows {
        let row: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM identities), \
             (SELECT count(*) FROM nodes WHERE parent_id IS NULL), (SELECT count(*) FROM account_feeds), \
             (SELECT count(*) FROM quota), (SELECT count(*) FROM invites WHERE redeemed_at IS NOT NULL)",
        )
        .fetch_one(&mut self.world.conn)
        .await
        .expect("count rows");
        Rows {
            accounts: row.0,
            identities: row.1,
            root_nodes: row.2,
            feeds: row.3,
            quotas: row.4,
            redeemed_invites: row.5,
        }
    }

    /// The account the identity `(provider, subject)` belongs to: its role and display name.
    pub async fn account_of(
        &mut self,
        provider: &str,
        subject: &str,
    ) -> Option<(Uuid, String, String)> {
        sqlx::query_as(
            "SELECT a.id, a.role, a.display_name FROM identities i JOIN accounts a ON a.id = i.account_id \
             WHERE i.provider = $1 AND i.provider_user_id = $2",
        )
        .bind(provider)
        .bind(subject)
        .fetch_optional(&mut self.world.conn)
        .await
        .expect("read the identity's account")
    }

    /// The hook rows of `account`: root nodes, feeds, quota rows.
    pub async fn hook_rows_of(&mut self, account: Uuid) -> (i64, i64, i64) {
        sqlx::query_as(
            "SELECT (SELECT count(*) FROM nodes WHERE account_id = $1 AND parent_id IS NULL), \
             (SELECT count(*) FROM account_feeds WHERE account_id = $1), \
             (SELECT count(*) FROM quota WHERE account_id = $1)",
        )
        .bind(account)
        .fetch_one(&mut self.world.conn)
        .await
        .expect("count hook rows")
    }

    /// `(redeemed_by, redeemed)` of invite `id`.
    pub async fn redemption_of(&mut self, id: Uuid) -> (Option<Uuid>, bool) {
        sqlx::query_as("SELECT redeemed_by, redeemed_at IS NOT NULL FROM invites WHERE id = $1")
            .bind(id)
            .fetch_one(&mut self.world.conn)
            .await
            .expect("read invite")
    }

    pub async fn finish(self) {
        let Rig { world, blob_root } = self;
        world.finish().await;
        blob_root.close().expect("remove the scratch blob root");
    }
}

pub fn real_hooks(harness: &Harness, blob_root: &Path) -> Vec<Arc<dyn AccountLifecycleHook>> {
    let root = blob_root.to_string_lossy().into_owned();
    let settings = load_settings(&[("SUNNYCLOUD_BLOB_ROOT", root.as_str())]);
    let blob = BlobstoreConfig::from_settings(&settings);
    let clock: Arc<dyn Clock> = harness.clock.clone();
    let api = harness.api.clone();
    let queue = Arc::new(GcQueue::new(&blob, api.clone(), clock.clone()).expect("gc queue"));
    let marker = Arc::new(FileEpochMarker::new(&blob).expect("epoch marker"));
    vec![
        Arc::new(PgQuotaAdmin::new(
            api.clone(),
            QuotaConfig::from_settings(&settings),
        )),
        Arc::new(PgFeedLifecycle::new(api.clone(), clock.clone(), marker)),
        Arc::new(PgTreeLifecycle::new(api, queue, clock)),
    ]
}

/// The provider callback of `subject` at `provider` for a sign-in that presented `invite`, with `change`
/// applied to the provider's grant; returns the callback and its binding, ready for `complete`.
pub async fn authorized(
    world: &World,
    provider: &str,
    subject: &str,
    invite: Option<&str>,
    change: impl FnOnce(&mut Grant),
) -> (contracts::CallbackParams, contracts::BrowserBinding) {
    let mut request = begin_request(provider, "/files");
    request.invite_token =
        invite.map(|token| RawToken::from_presented(TokenKind::Invite, token.to_owned()));
    let redirect = world.service.begin(request).await.expect("begin");
    let callback = world.idp.authorize_with(&redirect.url, subject, change);
    (params(callback), redirect.browser_binding)
}

/// `begin` with `invite`, the provider login of `subject` and `complete`.
pub async fn sign_in(
    world: &World,
    provider: &str,
    subject: &str,
    invite: Option<&str>,
    change: impl FnOnce(&mut Grant),
) -> Result<SignInOutcome, SignInError> {
    let (callback, binding) = authorized(world, provider, subject, invite, change).await;
    world
        .service
        .complete(provider, callback, &binding, HOST, CLIENT_IP)
        .await
}

/// A test double of the port that fails its first `fail_first` calls and then succeeds, writing nothing.
pub struct FaultyHook {
    calls: AtomicUsize,
    fail_first: usize,
}

impl FaultyHook {
    pub fn failing_once() -> Arc<FaultyHook> {
        Arc::new(FaultyHook {
            calls: AtomicUsize::new(0),
            fail_first: 1,
        })
    }

    /// A double that never fails: it only counts its calls.
    pub fn counting() -> Arc<FaultyHook> {
        Arc::new(FaultyHook {
            calls: AtomicUsize::new(0),
            fail_first: 0,
        })
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("injected hook fault")]
struct InjectedFault;

#[async_trait]
impl AccountLifecycleHook for FaultyHook {
    async fn initialize(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), LifecycleError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call < self.fail_first {
            Err(LifecycleError::Unexpected(Box::new(InjectedFault)))
        } else {
            Ok(())
        }
    }

    async fn erase_batch(
        &self,
        _account: Uuid,
        _limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        Ok(EraseProgress { remaining: 0 })
    }
}
