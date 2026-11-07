//! Sharing skeleton (AZ-3696): the `share_links` migration on a real PostgreSQL 18 (`DATABASE_URL`, a superuser
//! URL), row-level security under the real roles, and the sharing settings with the grant key rule.

mod cascade;
mod config;
mod harness;
mod migration;
mod rls;
mod settings;
