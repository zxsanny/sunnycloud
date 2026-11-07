//! The error mapper (E-21/c09): status table, `Retry-After` and extras, opaque unexpected errors, the
//! per-port conversion table and the uniform not-found answer.

mod cases;
mod extras;
mod not_found;
mod opaque;
mod port_table;
mod probe;
mod schema;
mod status;

pub(crate) use opaque::Capture;
