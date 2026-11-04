# Security Approach

- **Authentication:** GitHub and generic OIDC sign-in only (Google works through OIDC); no local passwords. Desktop daemons use a device-code flow. Sessions and devices can be revoked, and revocation takes effect within 5 s.
- **Abuse limits:** more than 10 failed sign-in attempts per minute per IP are rejected.
- **Transport and storage:** TLS on all connections. Non-vault data is encrypted at rest on the server.
- **Decoder isolation:** media decoders run in containers with no network, no credentials and no blob mount. The runner is hardened (non-dumpable, child subreaper, all descendants killed after every job, seccomp denying `ptrace` and `process_vm_*`) and restarts after any hostile-class outcome.
- **Database roles:** two roles. `sunnycloud_migrator` owns the schema and runs migrations; `sunnycloud` is the API runtime role (DML only, no DDL, no `BYPASSRLS`, row-level security enabled). The audit log is insert and select only for the runtime role. Workers and decoders have no database login.
- **Updates:** the desktop update manifest is signed offline (never by an automatic CI secret), pinned to two Ed25519 public keys (active and recovery), and carries a monotonic serial and an expiry time.
- **Vault:** opt-in end-to-end encryption. The client encrypts; the server sees only ciphertext and sizes. The key comes from a separate vault passphrase, with a one-time recovery key. Lost passphrase and recovery key mean unrecoverable data. Share links for vault files carry the key in the URL fragment, so the server never sees it.
- **Share links:** expiry (default 7 days), optional password, and revocation within 5 s.
- **Integrity:** end-to-end hash verification on every transfer.
- **Secrets:** provider client secrets and tokens come from environment or a secret store, never from git.
- **Open for research:** the vault cipher and key-derivation parameters, token format and lifetime, and the threat model for a multi-tenant installation.
