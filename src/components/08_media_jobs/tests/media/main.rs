//! Media tables, settings and errors (AZ-3839): the `jobs`, `derived` and `worker_status` migration on a real
//! PostgreSQL 18 (`DATABASE_URL`, a superuser URL), the media settings and the wire codes of the 08 errors.

mod config;
mod errors;
mod keys;
mod migration;
mod plans;
mod rls;
mod support;
mod system_scope_repo;
