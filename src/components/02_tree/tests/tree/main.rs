//! Tree skeleton (AZ-3706): settings, typed errors, the retry-once rule and the `nodes`/`revisions` migration;
//! folder create, rename, move and delete (AZ-3708); on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

mod config;
mod constraints;
mod errors;
mod keys;
mod migration;
mod mutation_concurrency;
mod mutation_delete;
mod mutation_moves;
mod mutation_names;
mod mutation_noop;
mod mutation_support;
mod retry;
mod rls;
mod support;
