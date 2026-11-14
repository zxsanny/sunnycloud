//! 20 vault service, compiled only with feature `vault` (v1.2 builds; module-layout exception 6).

use std::sync::Arc;

use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_db::Db;
use sunnycloud_vault_service::{PgVaultLifecycle, PgVaultService, VaultConfig};

use super::domain::Tree;
use crate::command_config::ServeSettings;
use crate::context::ContextBuilder;

pub struct Vault {
    pub service: Arc<PgVaultService>,
    pub lifecycle: Arc<PgVaultLifecycle>,
}

pub(super) fn register(builder: ContextBuilder) -> ContextBuilder {
    builder.register::<Vault, _, _>(|services| async move {
        let settings = services.get::<ServeSettings>().await?;
        let db = Db::clone(&*services.get::<Db>().await?);
        let audit = AuditWriter::clone(&*services.get::<AuditWriter>().await?);
        let clock = services.get::<dyn Clock>().await?;
        let tree = services.get::<Tree>().await?;
        let config = VaultConfig::from_settings(&settings)?;
        Ok(Arc::new(Vault {
            service: Arc::new(PgVaultService::new(
                db.clone(),
                tree.service.clone(),
                clock,
                audit,
                &config,
            )),
            lifecycle: Arc::new(PgVaultLifecycle::new(db)),
        }))
    })
}
