//! Ports of 06 sharing. The owner side takes the resolved account id; the visitor side takes the presented
//! token and the client address, never an account.

mod owner;
mod public;

pub use owner::{CreatedShare, ShareError, ShareInfo, ShareService};
pub use public::{
    AccessGrant, PublicShareError, PublicShareService, ShareContent, ShareDerived, ShareOpen,
};
