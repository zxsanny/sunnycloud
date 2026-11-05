//! Role verification at `Db::connect` (ADR-014).

use sqlx::{PgConnection, PgPool};

use crate::config::DbRole;
use crate::error::DbError;

/// The connected user is the requested role and, for the API role, can neither bypass nor alter what
/// row-level security protects.
pub(crate) async fn verify(pool: &PgPool, role: DbRole) -> Result<(), DbError> {
    let mut conn = pool.acquire().await?;
    let user: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&mut *conn)
        .await?;
    if user != role.user() {
        return Err(DbError::WrongRole(
            "the connected user is not the requested role",
        ));
    }
    if role == DbRole::Api
        && let Some(problem) = api_privilege_problem(&mut conn).await?
    {
        return Err(DbError::WrongRole(problem));
    }
    Ok(())
}

/// Why the current user may not serve as the API role, if it may not. The application schema is `public`
/// (`deploy/postgres/initdb.d/10-roles.sh`); a member of its owner role counts as the owner.
pub(crate) async fn api_privilege_problem(
    conn: &mut PgConnection,
) -> Result<Option<&'static str>, DbError> {
    let (superuser, bypass_rls, owns_schema): (bool, bool, bool) = sqlx::query_as(
        "SELECT r.rolsuper, r.rolbypassrls, pg_has_role(r.oid, n.nspowner, 'MEMBER') \
         FROM pg_roles r, pg_namespace n \
         WHERE r.rolname = current_user AND n.nspname = 'public'",
    )
    .fetch_one(conn)
    .await?;
    Ok(if superuser {
        Some("the API role is a superuser")
    } else if bypass_rls {
        Some("the API role has BYPASSRLS")
    } else if owns_schema {
        Some("the API role owns the application schema")
    } else {
        None
    })
}
