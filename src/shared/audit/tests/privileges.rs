//! AC-1 (append only for the API role, NFT-SEC-47) and AC-8 (reversible migration) on a real PostgreSQL 18, with
//! the real API role. The grants are read and exercised as `sunnycloud`, never as a superuser.

mod support;

use sc_db::{DbError, Migrator, RunError, Scope};
use support::TestDb;
use uuid::Uuid;

const INSERT: &str =
    "INSERT INTO audit_log (at, event) VALUES ('2026-11-05T12:00:00Z'::timestamptz, 'sign_in')";

/// Runs one statement as `sunnycloud` and returns its failure, if any.
async fn api_failure(db: &TestDb, statement: &'static str) -> Option<DbError> {
    let api = db.api().await;
    let result = api
        .run(Scope::Account(Uuid::nil()), async |tx| {
            sqlx::raw_sql(statement).execute(tx.conn()).await?;
            Ok::<_, DbError>(())
        })
        .await;
    match result {
        Ok(()) => None,
        Err(RunError::Closure(error)) => Some(error),
        Err(other) => panic!("unexpected failure kind for {statement}: {other}"),
    }
}

#[tokio::test]
async fn api_role_can_insert_and_select_but_not_update_delete_truncate_or_grant() {
    let db = TestDb::migrated().await;

    assert_eq!(api_failure(&db, INSERT).await, None);
    assert_eq!(
        api_failure(&db, "SELECT count(*) FROM audit_log").await,
        None
    );
    let denied = [
        "UPDATE audit_log SET event = 'sign_in'",
        "DELETE FROM audit_log",
        "TRUNCATE audit_log",
        "GRANT sunnycloud_migrator TO sunnycloud",
        "ALTER TABLE audit_log OWNER TO sunnycloud",
        "ALTER TABLE audit_log DISABLE ROW LEVEL SECURITY",
        "DROP TABLE audit_log",
        "CREATE OR REPLACE FUNCTION audit_log_purge(cutoff timestamptz) RETURNS bigint LANGUAGE sql AS 'SELECT 0::bigint'",
        "ALTER FUNCTION audit_log_purge(timestamptz) SECURITY INVOKER",
    ];
    for statement in denied {
        let failure = api_failure(&db, statement).await;

        assert!(
            matches!(&failure, Some(DbError::Other(text)) if text.starts_with("SQLSTATE 42501")),
            "{statement} -> {failure:?}"
        );
    }
    assert_eq!(db.admin_count("SELECT count(*) FROM audit_log").await, 1);
    db.drop_db().await;
}

/// PostgreSQL answers a GRANT from a role that holds some privilege on the object but no grant option with a
/// WARNING ("no privileges were granted") instead of an error: the statement succeeds and changes nothing.
#[tokio::test]
async fn grant_by_the_api_role_changes_nothing() {
    let db = TestDb::migrated().await;
    let before = grants(&db).await;

    for statement in [
        "GRANT UPDATE, DELETE, TRUNCATE ON audit_log TO sunnycloud",
        "GRANT SELECT ON audit_log TO PUBLIC",
        "GRANT ALL ON audit_log TO sunnycloud WITH GRANT OPTION",
        "GRANT EXECUTE ON FUNCTION audit_log_purge(timestamptz) TO PUBLIC",
    ] {
        assert_eq!(api_failure(&db, statement).await, None, "{statement}");
    }

    assert_eq!(grants(&db).await, before);
    assert_eq!(db.admin_count("SELECT count(*) FROM pg_class c, aclexplode(c.relacl) a WHERE c.relname = 'audit_log' AND a.grantee = 0").await, 0);
    assert_eq!(db.admin_count("SELECT count(*) FROM pg_proc p, aclexplode(p.proacl) a WHERE p.proname = 'audit_log_purge' AND a.grantee = 0").await, 0);
    assert_eq!(
        api_failure(&db, "UPDATE audit_log SET event = 'sign_in'")
            .await
            .map(|e| matches!(e, DbError::Other(t) if t.starts_with("SQLSTATE 42501"))),
        Some(true)
    );
    db.drop_db().await;
}

async fn grants(db: &TestDb) -> Vec<(String, String, bool)> {
    sqlx::query_as(
        "SELECT grantee::text, privilege_type::text, is_grantable = 'YES' \
         FROM information_schema.role_table_grants WHERE table_name = 'audit_log' ORDER BY 1, 2",
    )
    .fetch_all(&mut db.admin().await)
    .await
    .expect("grants")
}

#[tokio::test]
async fn table_is_owned_by_the_migrator_and_the_api_role_holds_only_insert_and_select() {
    let db = TestDb::migrated().await;
    let mut admin = db.admin().await;

    let owner: String = sqlx::query_scalar(
        "SELECT pg_get_userbyid(relowner)::text FROM pg_class WHERE oid = 'public.audit_log'::regclass",
    )
    .fetch_one(&mut admin)
    .await
    .expect("table owner");
    let privileges: Vec<String> = sqlx::query_scalar(
        "SELECT privilege_type::text FROM information_schema.role_table_grants \
         WHERE table_name = 'audit_log' AND grantee = 'sunnycloud' ORDER BY 1",
    )
    .fetch_all(&mut admin)
    .await
    .expect("table grants");
    let public_grants: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.role_table_grants \
         WHERE table_name = 'audit_log' AND grantee = 'PUBLIC'",
    )
    .fetch_one(&mut admin)
    .await
    .expect("public grants");
    let foreign_keys: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint WHERE conrelid = 'public.audit_log'::regclass AND contype = 'f'",
    )
    .fetch_one(&mut admin)
    .await
    .expect("foreign keys");
    let rls: bool = sqlx::query_scalar(
        "SELECT relrowsecurity FROM pg_class WHERE oid = 'public.audit_log'::regclass",
    )
    .fetch_one(&mut admin)
    .await
    .expect("row security flag");

    assert_eq!(owner, "sunnycloud_migrator");
    assert_eq!(privileges, ["INSERT", "SELECT"]);
    assert_eq!(public_grants, 0);
    assert_eq!(foreign_keys, 0);
    assert!(
        !rls,
        "audit_log is installation data without row-level security"
    );
    db.drop_db().await;
}

#[tokio::test]
async fn purge_function_is_security_definer_with_a_fixed_search_path_for_the_api_role_only() {
    let db = TestDb::migrated().await;
    let mut admin = db.admin().await;

    let (owner, definer, config): (String, bool, Option<Vec<String>>) = sqlx::query_as(
        "SELECT pg_get_userbyid(proowner)::text, prosecdef, proconfig \
         FROM pg_proc WHERE proname = 'audit_log_purge'",
    )
    .fetch_one(&mut admin)
    .await
    .expect("purge function");
    let api_can: bool = sqlx::query_scalar(
        "SELECT has_function_privilege('sunnycloud', 'audit_log_purge(timestamptz)', 'EXECUTE')",
    )
    .fetch_one(&mut admin)
    .await
    .expect("api privilege");
    let public_grants: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_proc p, aclexplode(p.proacl) a \
         WHERE p.proname = 'audit_log_purge' AND a.grantee = 0",
    )
    .fetch_one(&mut admin)
    .await
    .expect("public privilege");

    assert_eq!(owner, "sunnycloud_migrator");
    assert!(definer);
    assert_eq!(
        config,
        Some(vec!["search_path=pg_catalog, pg_temp".to_owned()])
    );
    assert!(api_can);
    assert_eq!(public_grants, 0);
    db.drop_db().await;
}

#[tokio::test]
async fn audit_migration_applies_reverts_and_applies_again() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let base = Migrator::run_pending(&migrator).await.expect("apply all");
    assert!(
        base.applied.len() >= 2,
        "accounts_base then audit: {base:?}"
    );
    let from_audit = base.applied[1..].to_vec();

    let reverted = Migrator::revert(
        &migrator,
        u32::try_from(from_audit.len()).expect("a few migrations"),
    )
    .await
    .expect("revert the audit migration and the ones after it");
    let gone = db
        .admin_count("SELECT count(*) FROM pg_class WHERE relname = 'audit_log'")
        .await;
    let function_gone = db
        .admin_count("SELECT count(*) FROM pg_proc WHERE proname = 'audit_log_purge'")
        .await;
    let again = Migrator::run_pending(&migrator).await.expect("apply again");

    let newest_first: Vec<i64> = from_audit.iter().rev().copied().collect();
    assert_eq!(reverted.reverted, newest_first);
    assert_eq!((gone, function_gone), (0, 0));
    assert_eq!(again.applied, from_audit);
    db.drop_db().await;
}

#[tokio::test]
async fn database_refuses_an_unknown_event_and_a_detail_over_2048_bytes() {
    let db = TestDb::migrated().await;

    let unknown = api_failure(
        &db,
        "INSERT INTO audit_log (at, event) VALUES ('2026-11-05T12:00:00Z'::timestamptz, 'login')",
    )
    .await;
    let big = api_failure(
        &db,
        "INSERT INTO audit_log (at, event, detail) VALUES ('2026-11-05T12:00:00Z'::timestamptz, 'sign_in', \
         jsonb_build_object('k', repeat('x', 2100)))",
    )
    .await;
    let limit = api_failure(
        &db,
        "INSERT INTO audit_log (at, event, detail) VALUES ('2026-11-05T12:00:00Z'::timestamptz, 'sign_in', \
         jsonb_build_object('k', repeat('x', 2048 - 9)))",
    )
    .await;

    assert_eq!(unknown, Some(DbError::CheckViolation));
    assert_eq!(big, Some(DbError::CheckViolation));
    assert_eq!(limit, None, "exactly 2048 bytes of jsonb text fit");
    db.drop_db().await;
}
