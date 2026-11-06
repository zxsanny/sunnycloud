//! Blobstore skeleton (AZ-3723): settings, typed errors and the `blobs`/`blob_gc` migration on a real
//! PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

mod config;
mod errors;
mod migration;
mod rls;
mod support;
