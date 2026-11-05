//! Integration tests against a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL). Each test clones its own
//! database from a shared template; see `support`.

mod faults;
mod lock;
mod proxy;
mod roles;
mod rollback;
mod scope;
mod support;
