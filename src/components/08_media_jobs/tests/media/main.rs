//! Media tables, settings, errors (AZ-3839) and the job queue (AZ-3840) on a real PostgreSQL 18
//! (`DATABASE_URL`, a superuser URL): the `jobs`, `derived` and `worker_status` migration, the media settings,
//! the wire codes of the 08 errors, and the enqueue, claim, lease, failure and maintenance statements.

mod config;
mod errors;
mod keys;
mod migration;
mod plans;
mod queue_claim;
mod queue_enqueue;
mod queue_fail;
mod queue_lease;
mod queue_maint;
mod queue_plan;
mod queue_support;
mod rls;
mod support;
mod system_scope_repo;
