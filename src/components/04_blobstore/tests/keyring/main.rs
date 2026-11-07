//! Master keyring (AZ-3725): key files, `.keycheck` on a real volume folder, `blobs` rows on a real PostgreSQL 18
//! (`DATABASE_URL`, a superuser URL), real XChaCha20-Poly1305 wraps.

mod init;
mod rotation;
mod start;
mod support;
mod wrap;
