//! What every component shares: the clock, the API database pool, the audit writer, the metrics registry, the
//! one `AbuseLimiter`, the shutdown signals, and the database readiness item (module-layout open point 4).

use std::sync::Arc;
use std::time::Duration;

use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_config::Settings;
use sc_db::{Db, DbConfig, DbRole};
use sc_observability::{BuildInfo, CheckFuture, Metrics, ReadinessCheck, ReadinessItem};
use sc_ratelimit::{AbuseLimiter, LimitId, LimitSpec, LimitTable};
use tokio_util::sync::CancellationToken;

use super::failed;
use crate::command_config::ServeSettings;
use crate::context::ContextBuilder;

const MINUTE: Duration = Duration::from_secs(60);
/// `InvalidTokenIp` when `invalid_token_ip_limit` is unset: 100 per minute (owner, batch 53). AC-20 gives no number
/// for `DeviceGrantIp` either, and no component checks that limit yet, so it takes the same default.
const IP_LIMIT_DEFAULT: u64 = 100;

/// The signals `serve` raises at shutdown. `feed` goes to 05's `ChangeFeed` at construction (contract part 2,
/// open point 5) and to `ApiServices::feed_shutdown`; `janitors` ends a running GC cycle between batches.
pub struct Shutdown {
    pub feed: CancellationToken,
    pub janitors: CancellationToken,
}

pub(super) fn register(builder: ContextBuilder, clock: Arc<dyn Clock>) -> ContextBuilder {
    builder
        .register::<dyn Clock, _, _>(move |_| {
            let clock = Arc::clone(&clock);
            async move { Ok(clock) }
        })
        .register::<Db, _, _>(|services| async move {
            let settings = services.get::<ServeSettings>().await?;
            let config = DbConfig::from_settings(&settings);
            let db = Db::connect(&config, DbRole::Api)
                .await
                .map_err(|error| failed("the API database pool", error))?;
            Ok(Arc::new(db))
        })
        .register::<AuditWriter, _, _>(|services| async move {
            let db = services.get::<Db>().await?;
            let clock = services.get::<dyn Clock>().await?;
            Ok(Arc::new(AuditWriter::new(Db::clone(&db), clock)))
        })
        .register::<Metrics, _, _>(|_| async {
            let metrics = Metrics::new(BuildInfo {
                version: env!("CARGO_PKG_VERSION"),
                commit: option_env!("SUNNYCLOUD_GIT_COMMIT").unwrap_or("unknown"),
                test_clock: cfg!(feature = "test-clock"),
            })
            .map_err(|error| failed("the metrics registry", error))?;
            Ok(Arc::new(metrics))
        })
        .register::<AbuseLimiter, _, _>(|services| async move {
            let settings = services.get::<ServeSettings>().await?;
            let clock = services.get::<dyn Clock>().await?;
            let limiter = AbuseLimiter::new(clock, limit_table(&settings))
                .map_err(|error| failed("the abuse limiter", error))?;
            Ok(Arc::new(limiter))
        })
        .register::<Shutdown, _, _>(|_| async {
            Ok(Arc::new(Shutdown {
                feed: CancellationToken::new(),
                janitors: CancellationToken::new(),
            }))
        })
        .register::<DatabaseReady, _, _>(|services| async move {
            let db = services.get::<Db>().await?;
            Ok(Arc::new(DatabaseReady { db: Db::clone(&db) }))
        })
}

/// The configured abuse limits on top of the AC-20 defaults of `LimitTable::new`. The IP limits without an
/// AC-20 number are counted per minute, like the other per-address limits.
fn limit_table(settings: &Settings) -> LimitTable {
    let per_minute = |count: u64| LimitSpec::new(u32::try_from(count).unwrap_or(u32::MAX), MINUTE);
    let invalid_token = per_minute(settings.invalid_token_ip_limit.unwrap_or(IP_LIMIT_DEFAULT));
    let device_grant = per_minute(settings.device_grant_ip_limit.unwrap_or(IP_LIMIT_DEFAULT));
    LimitTable::new(device_grant, invalid_token)
        .with_spec(
            LimitId::SharePasswordLink,
            LimitSpec::new(
                u32::try_from(settings.share_wrong_password_limit).unwrap_or(u32::MAX),
                settings.share_wrong_password_window.get(),
            ),
        )
        .with_spec(
            LimitId::ShareTokenIp,
            per_minute(settings.share_invalid_token_limit),
        )
        .with_max_entries(usize::try_from(settings.ratelimit_max_entries).unwrap_or(usize::MAX))
}

/// The `database` item of `/readyz`: Ok while helper 07's health probe reaches the database.
pub struct DatabaseReady {
    db: Db,
}

impl ReadinessCheck for DatabaseReady {
    fn name(&self) -> &str {
        "database"
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        Box::pin(async move {
            if self.db.health().await.reachable {
                ReadinessItem::ok()
            } else {
                ReadinessItem::failing("database unreachable")
            }
        })
    }
}
