//! Port doubles, the service harness and the settings loader. The skeleton doubles of `doubles` answer every
//! call with a typed failure that names the double; the SQL-backed doubles of `accounts_double` and
//! `quota_double` stand for 01 and 07a in the user-administration tests; `world` wires the real components
//! (dev-dependencies, ADR-015) for the account view and the deletion request.

#![allow(dead_code)]

pub mod account_data;
pub mod accounts_double;
pub mod doubles;
pub mod harness;
pub mod health_doubles;
pub mod quota_double;
pub mod seed;
pub mod settings;
pub mod world;
