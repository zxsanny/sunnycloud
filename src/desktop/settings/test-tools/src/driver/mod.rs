//! UI driver: launches the settings window and acts on it only through the OS accessibility
//! tree (AT-SPI on Linux, UI Automation on Windows). The same [`Session`] script runs on
//! both; only the [`Backend`] differs. There is deliberately no other way to reach a
//! control: no view-model access and no renderer hook.

mod session;

#[cfg(target_os = "linux")]
mod atspi;
#[cfg(windows)]
mod uia;

#[cfg(all(test, unix))]
mod tests;

use std::time::Duration;

pub use session::{LaunchSpec, Session, TreeEntry, dump_text};

#[cfg(target_os = "linux")]
pub use atspi::AtspiBackend as NativeBackend;
#[cfg(windows)]
pub use uia::UiaBackend as NativeBackend;

/// Control kinds, the same on both platforms; the platform's own role name is kept in
/// [`NodeInfo::native_role`] and shown by the dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Window,
    Dialog,
    Button,
    ToggleButton,
    CheckBox,
    RadioButton,
    TextInput,
    SpinButton,
    ComboBox,
    Tab,
    TabList,
    List,
    ListItem,
    Label,
    Table,
    Group,
    Other,
}

impl Role {
    /// Controls a user can act on; the window counts as interactive once one is in the tree.
    pub fn is_interactive(self) -> bool {
        matches!(
            self,
            Self::Button
                | Self::ToggleButton
                | Self::CheckBox
                | Self::RadioButton
                | Self::TextInput
                | Self::SpinButton
                | Self::ComboBox
                | Self::Tab
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    pub name: String,
    pub role: Role,
    pub native_role: String,
    /// Text of a text field, number of a spin box, `checked` / `unchecked` of a toggle.
    pub value: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("accessibility service unavailable: {0}")]
    Unavailable(String),
    #[error("accessibility call failed: {0}")]
    Call(String),
    #[error("cannot start the window `{program}`: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("the window process exited before it showed its tree: {0}")]
    WindowExited(String),
    #[error("no control named {name:?} with role {role:?} within {waited:?}")]
    NotFound {
        name: String,
        role: Role,
        waited: Duration,
    },
    #[error("{count} controls are named {name:?} with role {role:?}; the name is ambiguous")]
    Ambiguous {
        name: String,
        role: Role,
        count: usize,
    },
    #[error("the window showed no interactive control within {0:?}")]
    NotInteractive(Duration),
    #[error("the accessibility tree has more than {0} nodes")]
    TreeTooLarge(usize),
    #[error("the control does not support {0}")]
    Unsupported(&'static str),
}

/// One platform's accessibility API. Futures are deliberately not `Send`: UI Automation
/// objects are bound to their COM apartment, and the driver runs on a current-thread runtime.
#[allow(async_fn_in_trait)]
pub trait Backend {
    type Node: Clone;

    /// The root of the application owned by process `pid`; `None` until it has registered.
    async fn application(&self, pid: u32) -> Result<Option<Self::Node>, DriverError>;
    async fn children(&self, node: &Self::Node) -> Result<Vec<Self::Node>, DriverError>;
    async fn info(&self, node: &Self::Node) -> Result<NodeInfo, DriverError>;
    /// The control's default action (press, toggle, select).
    async fn click(&self, node: &Self::Node) -> Result<(), DriverError>;
    /// Enters `text` into an empty text field.
    async fn type_text(&self, node: &Self::Node, text: &str) -> Result<(), DriverError>;
}
