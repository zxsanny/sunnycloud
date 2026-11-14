//! Sharing on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL): the `share_links` migration, row-level
//! security under the real roles and the sharing settings with the grant key rule (AZ-3696), and the owner
//! operations create, list and revoke over the real tree and audit log (AZ-3697), and the visitor's `open` with its
//! constant-work lookup (AZ-3698), the visitor's `authorize_content` over files changed through the real 02
//! (AZ-3700), and the account erase hook (AZ-3702).

mod cascade;
mod config;
mod content_support;
mod harness;
mod lifecycle;
mod migration;
mod owner_create;
mod owner_manage;
mod owner_support;
mod rls;
mod settings;
mod visitor_content;
mod visitor_content_timing;
mod visitor_open;
mod visitor_statements;
mod visitor_support;
