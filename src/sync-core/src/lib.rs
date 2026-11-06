//! Native executor of the sunnycloud sync protocol: typed errors and the wire-code table so far.

mod error;
mod wire_table;

pub use error::{Mapped, PlannerInput, SyncError};
pub use wire_table::map_problem;
