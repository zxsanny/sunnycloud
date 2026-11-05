REVOKE EXECUTE ON FUNCTION audit_log_purge(timestamptz) FROM sunnycloud;
DROP FUNCTION audit_log_purge(timestamptz);

REVOKE INSERT, SELECT ON audit_log FROM sunnycloud;
DROP TABLE audit_log;
