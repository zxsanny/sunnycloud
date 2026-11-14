//! The 07 ports of the acceptance checks over the purger world's real components: 07's admin service (01's account
//! admin, 07a's quota admin) and deletion service, 01's authenticator, and 06's share service over 02's tree
//! reader. Fixtures for credentials that are live at the wall clock the purger world runs on.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use contracts::{Authenticator, Credential, Principal};
use sc_audit::AuditWriter;
use sc_config::Secret;
use sc_ratelimit::{AbuseLimiter, LimitId, LimitSpec, LimitTable};
use sqlx::PgConnection;
use sunnycloud_admin::{AccountDeletionServiceImpl, AdminServiceImpl};
use sunnycloud_identity::{IdentityAccounts, IdentityAuthenticator, IdentityConfig};
use sunnycloud_quota::{PgQuotaAdmin, QuotaConfig};
use sunnycloud_sharing::{PgShareService, ShareConfig};
use sunnycloud_tree::{PgTreeReader, TreeConfig};
use uuid::Uuid;

use crate::purge_support::world::PurgeWorld;

const ROOMY: LimitSpec = LimitSpec::new(10_000, Duration::from_secs(60));

/// The visitor address of the share link probes.
pub const VISITOR: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7));

pub struct Ports {
    pub admin: AdminServiceImpl,
    pub deletion: AccountDeletionServiceImpl,
    pub shares: PgShareService,
    identity: IdentityConfig,
}

impl Ports {
    pub fn new(world: &PurgeWorld) -> Ports {
        let api = world.api.clone();
        let clock = world.clock.clone();
        let accounts = Arc::new(IdentityAccounts::new(api.clone(), clock.clone()));
        let audit = AuditWriter::new(api.clone(), clock.clone());
        let admin = AdminServiceImpl::new(
            accounts.clone(),
            Arc::new(PgQuotaAdmin::new(
                api.clone(),
                QuotaConfig::from_settings(&world.settings),
            )),
            Vec::new(),
            world.config.clone(),
            audit.clone(),
            api.clone(),
        );
        let deletion = AccountDeletionServiceImpl::new(
            accounts,
            world.config.clone(),
            audit.clone(),
            api.clone(),
            clock.clone(),
        );
        let limiter = Arc::new(
            AbuseLimiter::new(
                clock.clone(),
                LimitTable::new(ROOMY, ROOMY)
                    .with_spec(LimitId::ShareTokenIp, ROOMY)
                    .with_spec(LimitId::SharePasswordLink, ROOMY),
            )
            .expect("limit table"),
        );
        let share_config = ShareConfig {
            grant_key: Secret::new("k".repeat(32)),
            ..ShareConfig::from_settings(&world.settings)
        };
        let tree = Arc::new(PgTreeReader::new(
            api.clone(),
            &TreeConfig::from_settings(&world.settings),
        ));
        let shares = PgShareService::new(api, clock, tree, audit, limiter, &share_config)
            .expect("share service");
        Ports {
            admin,
            deletion,
            shares,
            identity: IdentityConfig::from_settings(&world.settings),
        }
    }

    /// A new authenticator with an empty principal cache, as a freshly started API process has.
    pub fn authenticator(&self, world: &PurgeWorld) -> IdentityAuthenticator {
        IdentityAuthenticator::new(world.api.clone(), world.clock.clone(), &self.identity)
    }
}

/// A web session of `account` that is live for 100 years from the epoch; its access token is
/// `support::world::session_credential(id)`.
pub async fn live_session(conn: &mut PgConnection, account: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sessions (id, account_id, client_kind, label, token_hash, access_expires_at, \
         refresh_hash, refresh_expires_at, created_at) \
         VALUES ($1, $2, 'web', 'fixture', sha256(($1::text || 'a')::bytea), \
         'epoch'::timestamptz + interval '100 years', sha256(($1::text || 'r')::bytea), \
         'epoch'::timestamptz + interval '100 years', 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .execute(conn)
    .await
    .expect("insert live session");
    id
}

/// The principal 01 issues for `credential`.
pub async fn principal(authenticator: &IdentityAuthenticator, credential: Credential) -> Principal {
    authenticator
        .authenticate(credential)
        .await
        .expect("the credential authenticates")
}
