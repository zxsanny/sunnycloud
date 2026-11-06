use std::collections::VecDeque;
use std::ffi::OsString;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tokio::time::sleep;

use super::{Backend, DriverError, NodeInfo, Role};

const POLL: Duration = Duration::from_millis(25);
/// A window tree has tens of nodes; a cap this far above that only stops a cyclic tree.
const MAX_NODES: usize = 20_000;

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl LaunchSpec {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// One line of the tree dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub depth: usize,
    pub info: NodeInfo,
}

pub struct Session<B: Backend> {
    backend: B,
    child: Child,
    pid: u32,
    started: Instant,
}

impl<B: Backend> Session<B> {
    /// Starts the window. The process is killed when the session is dropped.
    pub fn launch(backend: B, spec: &LaunchSpec) -> Result<Self, DriverError> {
        let started = Instant::now();
        let mut command = Command::new(&spec.program);
        command.args(&spec.args).kill_on_drop(true);
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        let child = command.spawn().map_err(|source| DriverError::Spawn {
            program: spec.program.to_string_lossy().into_owned(),
            source,
        })?;
        let pid = child.id().ok_or_else(|| {
            DriverError::WindowExited("the process ended before it could be inspected".to_owned())
        })?;
        Ok(Self {
            backend,
            child,
            pid,
            started,
        })
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Time from the start of the process until its tree shows an interactive control.
    /// This is the accessibility-visible approximation of "first interactive frame" (PT-15-1):
    /// a rendered frame no assistive technology can see does not count.
    pub async fn first_interactive(&mut self, within: Duration) -> Result<Duration, DriverError> {
        let deadline = self.started + within;
        let mut last_call = None;
        loop {
            self.ensure_running()?;
            match self.snapshot().await {
                Ok(Some(entries)) if entries.iter().any(|e| e.1.role.is_interactive()) => {
                    return Ok(self.started.elapsed());
                }
                Ok(_) => {}
                Err(error @ DriverError::Call(_)) => last_call = Some(error),
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(last_call.unwrap_or(DriverError::NotInteractive(within)));
            }
            sleep(POLL).await;
        }
    }

    /// The one control with exactly this accessible name and role. Two matches are an error:
    /// the driver never picks one of them.
    pub async fn find(
        &mut self,
        name: &str,
        role: Role,
        within: Duration,
    ) -> Result<B::Node, DriverError> {
        let deadline = Instant::now() + within;
        let mut last_call = None;
        loop {
            self.ensure_running()?;
            match self.snapshot().await {
                Ok(Some(entries)) => {
                    let mut matches: Vec<B::Node> = entries
                        .into_iter()
                        .filter(|(_, info, _)| info.name == name && info.role == role)
                        .map(|(_, _, node)| node)
                        .collect();
                    match matches.len() {
                        0 => {}
                        1 => return Ok(matches.remove(0)),
                        count => {
                            return Err(DriverError::Ambiguous {
                                name: name.to_owned(),
                                role,
                                count,
                            });
                        }
                    }
                }
                Ok(None) => {}
                Err(error @ DriverError::Call(_)) => last_call = Some(error),
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(last_call.unwrap_or(DriverError::NotFound {
                    name: name.to_owned(),
                    role,
                    waited: within,
                }));
            }
            sleep(POLL).await;
        }
    }

    pub async fn click(&self, node: &B::Node) -> Result<(), DriverError> {
        self.backend.click(node).await
    }

    pub async fn type_text(&self, node: &B::Node, text: &str) -> Result<(), DriverError> {
        self.backend.type_text(node, text).await
    }

    pub async fn info(&self, node: &B::Node) -> Result<NodeInfo, DriverError> {
        self.backend.info(node).await
    }

    /// The control's current value (see [`NodeInfo::value`]); `None` when it has none.
    pub async fn value(&self, node: &B::Node) -> Result<Option<String>, DriverError> {
        Ok(self.backend.info(node).await?.value)
    }

    /// Every node of the window, depth first, with name and role.
    pub async fn dump(&mut self) -> Result<Vec<TreeEntry>, DriverError> {
        self.ensure_running()?;
        let entries = self.snapshot().await?.ok_or_else(|| {
            DriverError::Call("the window is not in the accessibility tree".to_owned())
        })?;
        Ok(entries
            .into_iter()
            .map(|(depth, info, _)| TreeEntry { depth, info })
            .collect())
    }

    /// Stops the window and waits for it.
    pub async fn close(mut self) -> Result<(), DriverError> {
        self.child
            .kill()
            .await
            .map_err(|e| DriverError::Call(format!("stopping the window: {e}")))
    }

    fn ensure_running(&mut self) -> Result<(), DriverError> {
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(status)) => Err(DriverError::WindowExited(status.to_string())),
            Err(error) => Err(DriverError::WindowExited(error.to_string())),
        }
    }

    /// The window's tree, or `None` while the application has not registered yet.
    async fn snapshot(&self) -> Result<Option<Vec<(usize, NodeInfo, B::Node)>>, DriverError> {
        match self.backend.application(self.pid).await? {
            Some(root) => self.walk(&root).await.map(Some),
            None => Ok(None),
        }
    }

    async fn walk(&self, root: &B::Node) -> Result<Vec<(usize, NodeInfo, B::Node)>, DriverError> {
        let mut found = Vec::new();
        let mut pending = VecDeque::from([(0_usize, root.clone())]);
        // Depth first: children are pushed to the front in reverse order.
        while let Some((depth, node)) = pending.pop_front() {
            if found.len() >= MAX_NODES {
                return Err(DriverError::TreeTooLarge(MAX_NODES));
            }
            let info = self.backend.info(&node).await?;
            let children = self.backend.children(&node).await?;
            for child in children.into_iter().rev() {
                pending.push_front((depth + 1, child));
            }
            found.push((depth, info, node));
        }
        Ok(found)
    }
}

/// The dump as text: one control per line, indented by depth, `Role "name" [native] = value`.
pub fn dump_text(entries: &[TreeEntry]) -> String {
    let mut text = String::new();
    for entry in entries {
        let info = &entry.info;
        text.push_str(&"  ".repeat(entry.depth));
        text.push_str(&format!(
            "{:?} {:?} [{}]",
            info.role, info.name, info.native_role
        ));
        if let Some(value) = &info.value {
            text.push_str(&format!(" = {value:?}"));
        }
        text.push('\n');
    }
    text
}
