//! Layer 8 of the stack of 11 section 5: credential extraction and the auth class of the route
//! (`api_v1_surface.md`, credential transport and auth classes).
//!
//! The class comes from the route's spec and is applied where the router is built, so a route cannot be mounted
//! without it. The layer reads headers only: a refused request never has its body polled.

mod format;
mod layer;

pub(crate) use format::bearer;
pub use format::{CredentialFormat, CredentialSource};
pub use layer::Authenticated;
pub(crate) use layer::protect;
