//! Integration tests against a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL). Each test clones its own
//! database from a shared template; see `support`.

mod accounts_base;
mod faults;
mod health;
mod interrupted;
mod lock;
mod proxy;
mod roles;
mod rollback;
mod schema_state;
mod scope;
pub(crate) mod seeded;
mod support;
