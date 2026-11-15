//! An account as 01 creates it, minus the identity row (01's sign-in needs a live provider): the account row, the
//! real hooks' `initialize` from the root's hook list in one transaction, and a browser session whose access token
//! the tests send as a bearer.

use std::time::{SystemTime, UNIX_EPOCH};

use contracts::LifecycleError;
use sc_db::Scope;
use sqlx::Connection;
use sunnycloud_server::root::AccountHooks;
use uuid::Uuid;

use super::Stack;

const DAY_MICROS: i64 = 86_400_000_000;

pub struct Account {
    pub id: Uuid,
    pub root: Uuid,
    /// The session access token, sent as `Authorization: Bearer`.
    pub token: String,
}

fn now_micros() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after 1970");
    i64::try_from(since.as_micros()).expect("micros fit i64")
}

impl Stack {
    pub async fn account(&self) -> Account {
        let id = Uuid::new_v4();
        let at = now_micros();
        let mut admin = self.root.db.admin().await;
        sqlx::query(
            "INSERT INTO accounts (id, state, created_at, display_name, role) \
             VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Serve', 'user')",
        )
        .bind(id)
        .bind(at)
        .execute(&mut admin)
        .await
        .expect("insert account");
        let hooks = self
            .root
            .services
            .get::<AccountHooks>()
            .await
            .expect("hook list");
        self.root
            .db
            .api()
            .await
            .run(Scope::Account(id), async |tx| {
                for hook in &hooks.0 {
                    hook.item.initialize(tx, id).await?;
                }
                Ok::<_, LifecycleError>(())
            })
            .await
            .expect("the hooks initialize the account");
        let root: Uuid =
            sqlx::query_scalar("SELECT id FROM nodes WHERE account_id = $1 AND parent_id IS NULL")
                .bind(id)
                .fetch_one(&mut admin)
                .await
                .expect("the account's root");
        let token = format!("sc_ses_serve{}", Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO sessions (id, account_id, client_kind, label, token_hash, access_expires_at, \
             refresh_hash, refresh_expires_at, created_at) \
             VALUES (gen_random_uuid(), $1, 'web', 'serve test', sha256(convert_to($2, 'UTF8')), \
             'epoch'::timestamptz + $3::bigint * interval '1 microsecond', \
             sha256(convert_to($2 || '-refresh', 'UTF8')), \
             'epoch'::timestamptz + $3::bigint * interval '1 microsecond', \
             'epoch'::timestamptz + $4::bigint * interval '1 microsecond')",
        )
        .bind(id)
        .bind(&token)
        .bind(at + DAY_MICROS)
        .bind(at)
        .execute(&mut admin)
        .await
        .expect("insert session");
        admin.close().await.expect("close admin connection");
        Account { id, root, token }
    }
}
