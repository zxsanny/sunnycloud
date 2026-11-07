//! Port doubles, the service harness and the settings loader. The skeleton doubles of `doubles` answer every
//! call with a typed failure that names the double; the SQL-backed doubles of `accounts_double` and
//! `quota_double` stand for components the 07 crate may not link.

#![allow(dead_code)]

pub mod accounts_double;
pub mod doubles;
pub mod harness;
pub mod quota_double;
pub mod seed;
pub mod settings;
