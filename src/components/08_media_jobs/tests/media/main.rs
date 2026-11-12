//! Media tables, settings, errors (AZ-3839) and the job queue (AZ-3840) on a real PostgreSQL 18
//! (`DATABASE_URL`, a superuser URL): the `jobs`, `derived` and `worker_status` migration, the media settings,
//! the wire codes of the 08 errors, the enqueue, claim, lease, failure and maintenance statements, and the job
//! planning effect at commit (AZ-3841), also inside the real 02 `publish`.

mod config;
mod effect_plan;
mod effect_priority;
mod effect_rollback;
mod effect_skip;
mod effect_support;
mod errors;
mod keys;
mod migration;
mod plans;
mod publish_support;
mod queue_claim;
mod queue_enqueue;
mod queue_fail;
mod queue_lease;
mod queue_maint;
mod queue_plan;
mod queue_support;
mod rls;
mod statements;
mod support;
mod system_scope_repo;
