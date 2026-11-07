//! Blob rows (AZ-3729): `BlobRegistry`, `BlobCatalog` and `GcQueue` against a real PostgreSQL 18 and real files.
//! Callers' transactions come from `Db::run` as the API role `sunnycloud`; rows are probed as the schema owner.

mod catalog;
mod pending;
mod registry;
mod reinstate;
mod schedule;
mod support;
