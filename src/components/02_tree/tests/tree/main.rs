//! Tree skeleton (AZ-3706): settings, typed errors, the retry-once rule and the `nodes`/`revisions` migration;
//! folder create, rename, move and delete (AZ-3708); `TreeCommit::publish` (AZ-3709); the account lifecycle hook
//! and the blob owner cleanup (AZ-3717, AZ-3925 AC-4); the cascade and purge janitors (AZ-3714); on a real
//! PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

mod config;
mod constraints;
mod errors;
mod keys;
mod lifecycle_cleanup;
mod lifecycle_erase;
mod lifecycle_support;
mod lifecycle_vault;
mod maintenance_cascade;
mod maintenance_janitor;
mod maintenance_purge;
mod maintenance_support;
mod migration;
mod mutation_concurrency;
mod mutation_delete;
mod mutation_moves;
mod mutation_names;
mod mutation_noop;
mod mutation_support;
mod publish_batch;
mod publish_concurrency;
mod publish_outcomes;
mod publish_protocol;
mod publish_support;
mod retry;
mod rls;
mod statements;
mod support;
