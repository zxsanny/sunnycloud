"""The fixed read-only queries of the e2e-ctl action `db-probe` (environment.md, Read-only DB probe): no free-form SQL."""

# The fixed queries of `db-probe`: each returns one row of integers, named by the keys, or (kind `list`) one text of
# comma-separated values.
LIST = "list"
PROBES = {
    # The rows an account has after sign-in: the account, its identity and the rows the lifecycle hooks create.
    "account_rows": (("accounts", "identities", "quota", "account_feeds"),
                     "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM identities), "
                     "(SELECT count(*) FROM quota), (SELECT count(*) FROM account_feeds)"),
    # The security events the sign-in and session rules write.
    "audit_events": (("sign_in", "abuse_trip", "refresh_reuse", "session_revoked", "share_created", "share_revoked",
                      "share_accessed", "account_deleted"),
                     "SELECT count(*) FILTER (WHERE event = 'sign_in'), count(*) FILTER (WHERE event = 'abuse_trip'), "
                     "count(*) FILTER (WHERE event = 'refresh_reuse'), count(*) FILTER (WHERE event = 'session_revoked'), "
                     "count(*) FILTER (WHERE event = 'share_created'), count(*) FILTER (WHERE event = 'share_revoked'), "
                     "count(*) FILTER (WHERE event = 'share_accessed'), count(*) FILTER (WHERE event = 'account_deleted') "
                     "FROM audit_log"),
    # Rows per table that an account's data lives in: after an account is erased none of them keeps a row of it.
    "table_rows": (("accounts", "identities", "sessions", "device_tokens", "nodes", "revisions", "blobs", "blob_gc",
                    "upload_sessions", "share_links", "jobs", "derived", "quota", "account_feeds", "login_codes"),
                   "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM identities), (SELECT count(*) FROM sessions), "
                   "(SELECT count(*) FROM device_tokens), (SELECT count(*) FROM nodes), (SELECT count(*) FROM revisions), "
                   "(SELECT count(*) FROM blobs), (SELECT count(*) FROM blob_gc), (SELECT count(*) FROM upload_sessions), "
                   "(SELECT count(*) FROM share_links), (SELECT count(*) FROM jobs), (SELECT count(*) FROM derived), "
                   "(SELECT count(*) FROM quota), (SELECT count(*) FROM account_feeds), (SELECT count(*) FROM login_codes)"),
    # The media pipeline (AZ-3661): job states and outcomes, derived rows by kind, blobs that nothing refers to (neither a
    # revision nor a derived row: a surplus derived blob), and the supervisor's own status rows.
    "media_jobs": (("queued", "running", "derived", "skipped_compliant", "obsolete", "dead"),
                   "SELECT count(*) FILTER (WHERE status = 'queued'), count(*) FILTER (WHERE status = 'running'), "
                   "count(*) FILTER (WHERE status = 'succeeded' AND outcome = 'derived'), "
                   "count(*) FILTER (WHERE status = 'succeeded' AND outcome = 'skipped_compliant'), "
                   "count(*) FILTER (WHERE status = 'succeeded' AND outcome = 'obsolete'), "
                   "count(*) FILTER (WHERE status = 'dead') FROM jobs"),
    "job_attempts": (("most",), "SELECT coalesce(max(attempts), 0) FROM jobs"),
    "derived_rows": (("thumb", "preview", "transcode", "total"),
                     "SELECT count(*) FILTER (WHERE kind = 'thumb'), count(*) FILTER (WHERE kind = 'preview'), "
                     "count(*) FILTER (WHERE kind = 'transcode'), count(*) FROM derived"),
    "blob_refs": (("blobs", "committed", "unreferenced"),
                  "SELECT count(*), count(*) FILTER (WHERE state = 'committed'), count(*) FILTER (WHERE state = 'committed' AND "
                  "NOT EXISTS (SELECT 1 FROM revisions r WHERE r.blob_id = blobs.id) AND "
                  "NOT EXISTS (SELECT 1 FROM derived d WHERE d.blob_id = blobs.id)) FROM blobs"),
    "worker_status": (("workers", "decoder_ok"),
                      "SELECT count(*), count(*) FILTER (WHERE decoder_ok) FROM worker_status"),
    "account_states": (("active", "deleting"),
                       "SELECT count(*) FILTER (WHERE state = 'active'), count(*) FILTER (WHERE state = 'deleting') FROM accounts"),
    # What a share link row stores: the column names show a token hash and no token.
    "share_link_columns": (LIST,
                           "SELECT coalesce(string_agg(column_name, ',' ORDER BY column_name), '') "
                           "FROM information_schema.columns WHERE table_name = 'share_links'"),
    # The distinct keys of the detail of those events: the audit log holds ids, codes and integers, so a key that
    # names a secret or a person would show here.
    "audit_detail_keys": (LIST,
                          "SELECT coalesce(string_agg(k, ',' ORDER BY k), '') FROM (SELECT DISTINCT jsonb_object_keys(detail) AS k "
                          "FROM audit_log WHERE detail IS NOT NULL AND jsonb_typeof(detail) = 'object') s"),
    # What a destructive operator command changes besides rows: the applied migrations and the feed epochs (AZ-3815 AC-6).
    "schema_and_epochs": (("migrations_applied", "epoch_sum", "epoch_max"),
                          "SELECT (SELECT count(*) FROM _sqlx_migrations WHERE success), "
                          "(SELECT coalesce(sum(epoch), 0) FROM account_feeds), (SELECT coalesce(max(epoch), 0) FROM account_feeds)"),
}
