//! Blob maintenance ports (AZ-3746): rewrap, the `.epoch` marker, the volume health probe, the missing-blob
//! inventory and the doctor checks, on real files and a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

#[allow(dead_code)]
#[path = "../scrub/support.rs"]
mod support;

mod doctor;
mod epoch;
mod epoch_kill;
mod fixture;
mod health;
mod inventory;
mod restore;
mod rewrap;
