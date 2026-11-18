//! Media tables, settings, errors (AZ-3839) and the job queue (AZ-3840) on a real PostgreSQL 18
//! (`DATABASE_URL`, a superuser URL): the `jobs`, `derived` and `worker_status` migration, the media settings,
//! the wire codes of the 08 errors, the enqueue, claim, lease, failure and maintenance statements, and the job
//! planning effect at commit (AZ-3841), also inside the real 02 `publish`, and the thumbnail states and derived
//! lookups (AZ-3844), the acceptance of worker outputs and the worker input (AZ-3842), and the worker
//! authentication and the `WorkerJobs` facade (AZ-3843), the erase hook, blob owner cleanup and janitors
//! (AZ-3845), and the operator port, worker presence and media metrics (AZ-3846), and the jobs of a collected revision (AZ-4332).

mod accept_doubles;
mod accept_input;
mod accept_once;
mod accept_refuse;
mod accept_store;
mod accept_support;
mod admin_health;
mod blob_support;
mod config;
mod derived_open;
mod derived_support;
mod effect_plan;
mod effect_priority;
mod effect_rollback;
mod effect_shared;
mod effect_skip;
mod effect_support;
mod erase_hook;
mod errors;
mod janitors;
mod keys;
mod maint_support;
mod metrics_scrape;
mod migration;
mod obsolete;
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
mod support;
mod system_scope_repo;
mod thumb_states;
mod worker_facade;
mod worker_rotation;
mod worker_status;
