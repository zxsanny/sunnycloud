//! Linux backend: AT-SPI over the accessibility bus, the tree a screen reader reads.
//!
//! The AT-SPI client crates depend on zbus 5, whose `zbus_names` / `zvariant` split is flagged
//! unmaintained by the advisory gate (RUSTSEC-2026-0370..0372), so the few calls needed are
//! made directly on zbus 6. Role numbers, state bits and interface names are those of the
//! AT-SPI D-Bus specification (`org.a11y.atspi.*`).
//!
//! Typing uses the registry's keyboard synthesis after `GrabFocus`, because the toolkit under
//! test (Slint through AccessKit) offers no `EditableText`.

use std::fmt::Display;

use zbus::proxy::{Builder, CacheProperties};
use zbus::{Connection, Proxy};

use super::{Backend, DriverError, NodeInfo, Role};

const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const NULL_PATH: &str = "/org/a11y/atspi/null";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const ACTION: &str = "org.a11y.atspi.Action";
const COMPONENT: &str = "org.a11y.atspi.Component";
const TEXT: &str = "org.a11y.atspi.Text";
const VALUE: &str = "org.a11y.atspi.Value";
const KEY_SYNTH_STRING: u32 = 4;
const STATE_CHECKED: u32 = 4;

/// Action names that mean "press this control", in the order they are preferred.
const DEFAULT_ACTIONS: &[&str] = &["click", "press", "activate", "toggle", "invoke", "select"];

/// An AT-SPI object: the unique bus name of its application and its object path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtspiNode {
    bus_name: String,
    path: String,
}

pub struct AtspiBackend {
    bus: Connection,
}

fn unavailable(error: impl Display) -> DriverError {
    DriverError::Unavailable(error.to_string())
}

fn call(error: impl Display) -> DriverError {
    DriverError::Call(error.to_string())
}

impl AtspiBackend {
    /// Connects to the accessibility bus of the current session and switches session
    /// accessibility on, so a toolkit that exports its tree only on demand does so.
    pub async fn connect() -> Result<Self, DriverError> {
        let session = Connection::session().await.map_err(unavailable)?;
        let status = plain_proxy(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status")
            .await
            .map_err(unavailable)?;
        if !status
            .get_property::<bool>("IsEnabled")
            .await
            .map_err(unavailable)?
        {
            status
                .set_property("IsEnabled", true)
                .await
                .map_err(unavailable)?;
        }
        let bus = plain_proxy(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
            .await
            .map_err(unavailable)?;
        let address: String = bus.call("GetAddress", &()).await.map_err(unavailable)?;
        let bus = zbus::connection::Builder::address(address.as_str())
            .build()
            .await
            .map_err(unavailable)?;
        Ok(Self { bus })
    }

    async fn proxy<'a>(
        &self,
        node: &'a AtspiNode,
        interface: &'static str,
    ) -> Result<Proxy<'a>, DriverError> {
        plain_proxy(
            &self.bus,
            node.bus_name.as_str(),
            node.path.as_str(),
            interface,
        )
        .await
        .map_err(call)
    }
}

/// No property cache: the registry implements the interfaces only partly, and the tree
/// changes under the driver.
async fn plain_proxy<'a>(
    conn: &Connection,
    destination: &'a str,
    path: &'a str,
    interface: &'static str,
) -> zbus::Result<Proxy<'a>> {
    Builder::new(conn)
        .destination(destination)
        .path(path)
        .interface(interface)
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

fn map_role(role: u32) -> Role {
    match role {
        23 | 69 => Role::Window,
        16 | 2 => Role::Dialog,
        43 => Role::Button,
        62 => Role::ToggleButton,
        7 => Role::CheckBox,
        44 => Role::RadioButton,
        79 | 40 | 61 => Role::TextInput,
        52 => Role::SpinButton,
        11 => Role::ComboBox,
        37 => Role::Tab,
        38 => Role::TabList,
        31 | 98 => Role::List,
        32 => Role::ListItem,
        29 | 116 => Role::Label,
        55 => Role::Table,
        39 | 20 | 99 | 85 => Role::Group,
        _ => Role::Other,
    }
}

impl Backend for AtspiBackend {
    type Node = AtspiNode;

    async fn application(&self, pid: u32) -> Result<Option<AtspiNode>, DriverError> {
        let root = AtspiNode {
            bus_name: REGISTRY.to_owned(),
            path: ROOT_PATH.to_owned(),
        };
        let apps = self.children(&root).await?;
        let bus = zbus::fdo::DBusProxy::new(&self.bus)
            .await
            .map_err(unavailable)?;
        let mut identified = 0_usize;
        let mut last_error = None;
        for app in apps {
            let name = zbus::names::BusName::try_from(app.bus_name.as_str()).map_err(call)?;
            match bus.get_connection_unix_process_id(name).await {
                Ok(owner) if owner == pid => return Ok(Some(app)),
                Ok(_) => identified += 1,
                // An application that exited between the listing and this call has no owner;
                // it only matters when no application could be identified at all.
                Err(error) => last_error = Some(error),
            }
        }
        match last_error {
            Some(error) if identified == 0 => Err(call(error)),
            _ => Ok(None),
        }
    }

    async fn children(&self, node: &AtspiNode) -> Result<Vec<AtspiNode>, DriverError> {
        let proxy = self.proxy(node, ACCESSIBLE).await?;
        let refs: Vec<(String, zbus::OwnedObjectPath)> =
            proxy.call("GetChildren", &()).await.map_err(call)?;
        Ok(refs
            .into_iter()
            .filter(|(_, path)| path.as_str() != NULL_PATH)
            .map(|(bus_name, path)| AtspiNode {
                bus_name,
                path: path.as_str().to_owned(),
            })
            .collect())
    }

    async fn info(&self, node: &AtspiNode) -> Result<NodeInfo, DriverError> {
        let proxy = self.proxy(node, ACCESSIBLE).await?;
        let name: String = proxy.get_property("Name").await.map_err(call)?;
        let native: u32 = proxy.call("GetRole", &()).await.map_err(call)?;
        let interfaces: Vec<String> = proxy.call("GetInterfaces", &()).await.map_err(call)?;
        let role = map_role(native);
        let has = |interface: &str| interfaces.iter().any(|i| i == interface);
        let value = match role {
            Role::CheckBox | Role::RadioButton | Role::ToggleButton => {
                let states: Vec<u32> = proxy.call("GetState", &()).await.map_err(call)?;
                let checked = states
                    .first()
                    .is_some_and(|bits| bits & (1 << STATE_CHECKED) != 0);
                Some(if checked { "checked" } else { "unchecked" }.to_owned())
            }
            Role::TextInput if has(TEXT) => {
                let text = self.proxy(node, TEXT).await?;
                let count: i32 = text.get_property("CharacterCount").await.map_err(call)?;
                Some(text.call("GetText", &(0_i32, count)).await.map_err(call)?)
            }
            _ if has(VALUE) => {
                let value = self.proxy(node, VALUE).await?;
                let current: f64 = value.get_property("CurrentValue").await.map_err(call)?;
                Some(current.to_string())
            }
            _ => None,
        };
        Ok(NodeInfo {
            name,
            role,
            native_role: format!("atspi-role-{native}"),
            value,
        })
    }

    async fn click(&self, node: &AtspiNode) -> Result<(), DriverError> {
        let action = self.proxy(node, ACTION).await?;
        let count: i32 = action
            .get_property("NActions")
            .await
            .map_err(|_| DriverError::Unsupported("the Action interface"))?;
        if count <= 0 {
            return Err(DriverError::Unsupported("any action"));
        }
        let mut index = 0;
        for candidate in 0..count {
            let name: String = action.call("GetName", &(candidate,)).await.map_err(call)?;
            if DEFAULT_ACTIONS.contains(&name.to_ascii_lowercase().as_str()) {
                index = candidate;
                break;
            }
        }
        let done: bool = action.call("DoAction", &(index,)).await.map_err(call)?;
        if done {
            Ok(())
        } else {
            Err(DriverError::Call(
                "the control refused its action".to_owned(),
            ))
        }
    }

    async fn type_text(&self, node: &AtspiNode, text: &str) -> Result<(), DriverError> {
        let component = self.proxy(node, COMPONENT).await?;
        let focused: bool = component
            .call("GrabFocus", &())
            .await
            .map_err(|_| DriverError::Unsupported("focusing (the Component interface)"))?;
        if !focused {
            return Err(DriverError::Call(
                "the control refused the focus".to_owned(),
            ));
        }
        let keys = plain_proxy(
            &self.bus,
            REGISTRY,
            "/org/a11y/atspi/registry/deviceeventcontroller",
            "org.a11y.atspi.DeviceEventController",
        )
        .await
        .map_err(unavailable)?;
        keys.call::<_, _, ()>("GenerateKeyboardEvent", &(0_i32, text, KEY_SYNTH_STRING))
            .await
            .map_err(call)
    }
}
