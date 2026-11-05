-- Owner: shared/audit (domain word audit). Append-only security log of ids, codes and integers (data_model 10.1,
-- ADR-014). The table is owned by the role that runs the migration (sunnycloud_migrator). The API role gets
-- INSERT and SELECT only: it cannot update, delete, truncate or re-grant, because it owns nothing. There are no
-- foreign keys, so deleting an account keeps its history. No row-level security: installation data, read by
-- admin routes only. The application binds `at`; there is no default.
CREATE TABLE audit_log (
    id bigint GENERATED ALWAYS AS IDENTITY,
    at timestamptz NOT NULL,
    actor_account_id uuid,
    actor_session_id uuid,
    event text NOT NULL,
    subject_type text,
    subject_id uuid,
    ip inet,
    detail jsonb,
    CONSTRAINT audit_log_pkey PRIMARY KEY (id),
    CONSTRAINT audit_log_event_check CHECK (event IN (
        'sign_in', 'device_approved', 'device_revoked', 'session_revoked', 'refresh_reuse',
        'share_created', 'share_revoked', 'share_accessed', 'admin_action', 'account_deleted',
        'abuse_trip', 'vault_failure', 'scrub_mismatch', 'scrub_summary'
    )),
    CONSTRAINT audit_log_detail_size_check CHECK (octet_length(detail::text) <= 2048)
);

CREATE INDEX audit_log_at_idx ON audit_log (at);
CREATE INDEX audit_log_actor_at_idx ON audit_log (actor_account_id, at);

GRANT INSERT, SELECT ON audit_log TO sunnycloud;

-- Retention is the only delete. SECURITY DEFINER runs with the owner's rights, so the API role can remove rows
-- older than a cutoff and nothing else. search_path is fixed and every object is schema-qualified, so a caller
-- cannot redirect the function to its own objects. A new event kind swaps audit_log_event_check in a new
-- migration (additive), never here.
CREATE FUNCTION audit_log_purge(cutoff timestamptz) RETURNS bigint
    LANGUAGE plpgsql
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    removed bigint;
BEGIN
    DELETE FROM public.audit_log WHERE at < cutoff;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END
$$;

REVOKE ALL ON FUNCTION audit_log_purge(timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION audit_log_purge(timestamptz) TO sunnycloud;
