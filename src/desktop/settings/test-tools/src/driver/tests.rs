//! The driver's own logic (search, ambiguity, waiting, dump) against an in-memory tree.
//! The tree is a stand-in for the OS, so these tests prove nothing about AT-SPI or UI
//! Automation; AC-7 is proven by `tests/window_driver.rs` against the real window.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use super::*;

struct FakeNode {
    info: NodeInfo,
    children: Vec<usize>,
}

struct FakeBackend {
    nodes: RefCell<Vec<FakeNode>>,
    /// `application` answers `None` this many times before the window "registers".
    registers_after: Cell<u32>,
    clicked: RefCell<Vec<usize>>,
}

fn info(name: &str, role: Role, value: Option<&str>) -> NodeInfo {
    NodeInfo {
        name: name.to_owned(),
        role,
        native_role: format!("native-{role:?}"),
        value: value.map(str::to_owned),
    }
}

impl FakeBackend {
    /// window "Settings" > [button "Apply", text input "Server address", group > button "Apply"]
    fn window(registers_after: u32) -> Self {
        let node = |name, role, children: Vec<usize>| FakeNode {
            info: info(name, role, None),
            children,
        };
        Self {
            nodes: RefCell::new(vec![
                node("Settings", Role::Window, vec![1, 2, 3]),
                node("Apply", Role::Button, vec![]),
                FakeNode {
                    info: info("Server address", Role::TextInput, Some("")),
                    children: vec![],
                },
                node("Advanced", Role::Group, vec![4]),
                node("Apply", Role::Group, vec![]),
            ]),
            registers_after: Cell::new(registers_after),
            clicked: RefCell::new(Vec::new()),
        }
    }
}

impl Backend for FakeBackend {
    type Node = usize;

    async fn application(&self, _pid: u32) -> Result<Option<usize>, DriverError> {
        let left = self.registers_after.get();
        if left > 0 {
            self.registers_after.set(left - 1);
            return Ok(None);
        }
        Ok(Some(0))
    }

    async fn children(&self, node: &usize) -> Result<Vec<usize>, DriverError> {
        Ok(self.nodes.borrow()[*node].children.clone())
    }

    async fn info(&self, node: &usize) -> Result<NodeInfo, DriverError> {
        Ok(self.nodes.borrow()[*node].info.clone())
    }

    async fn click(&self, node: &usize) -> Result<(), DriverError> {
        self.clicked.borrow_mut().push(*node);
        Ok(())
    }

    async fn type_text(&self, node: &usize, text: &str) -> Result<(), DriverError> {
        self.nodes.borrow_mut()[*node].info.value = Some(text.to_owned());
        Ok(())
    }
}

/// A real, long-lived process stands for the window; the fake tree stands for its tree.
fn running_window() -> LaunchSpec {
    let mut spec = LaunchSpec::new("sleep");
    spec.args.push("30".into());
    spec
}

const WAIT: Duration = Duration::from_millis(300);

#[tokio::test]
async fn find_click_type_and_read_go_through_the_backend() {
    let mut session = Session::launch(FakeBackend::window(0), &running_window()).unwrap();

    let button = session.find("Apply", Role::Button, WAIT).await.unwrap();
    session.click(&button).await.unwrap();
    let field = session
        .find("Server address", Role::TextInput, WAIT)
        .await
        .unwrap();
    session
        .type_text(&field, "https://cloud.example")
        .await
        .unwrap();
    let value = session.value(&field).await.unwrap();

    assert_eq!(value.as_deref(), Some("https://cloud.example"));
    assert_eq!(session.backend().clicked.borrow().as_slice(), &[1]);
}

#[tokio::test]
async fn the_same_name_with_another_role_is_not_a_second_match() {
    let mut session = Session::launch(FakeBackend::window(0), &running_window()).unwrap();

    let group = session.find("Apply", Role::Group, WAIT).await;

    assert!(group.is_ok());
}

#[tokio::test]
async fn two_controls_with_one_name_and_role_are_ambiguous_not_first_wins() {
    let backend = FakeBackend::window(0);
    backend.nodes.borrow_mut()[4].info.role = Role::Button;
    let mut session = Session::launch(backend, &running_window()).unwrap();

    let outcome = session.find("Apply", Role::Button, WAIT).await;

    assert!(matches!(
        outcome,
        Err(DriverError::Ambiguous { count: 2, .. })
    ));
}

#[tokio::test]
async fn a_missing_control_is_not_found_after_the_wait() {
    let mut session = Session::launch(FakeBackend::window(0), &running_window()).unwrap();

    let outcome = session
        .find("Sign out", Role::Button, Duration::from_millis(100))
        .await;

    assert!(matches!(outcome, Err(DriverError::NotFound { .. })));
}

#[tokio::test]
async fn first_interactive_waits_for_the_window_to_register() {
    let mut session = Session::launch(FakeBackend::window(4), &running_window()).unwrap();

    let elapsed = session
        .first_interactive(Duration::from_secs(2))
        .await
        .unwrap();

    assert!(elapsed >= Duration::from_millis(75), "{elapsed:?}");
}

#[tokio::test]
async fn first_interactive_fails_when_no_control_is_interactive() {
    let backend = FakeBackend::window(0);
    for node in backend.nodes.borrow_mut().iter_mut() {
        node.info.role = Role::Label;
    }
    let mut session = Session::launch(backend, &running_window()).unwrap();

    let outcome = session.first_interactive(Duration::from_millis(100)).await;

    assert!(matches!(outcome, Err(DriverError::NotInteractive(_))));
}

#[tokio::test]
async fn the_dump_lists_every_control_with_name_role_and_depth() {
    let mut session = Session::launch(FakeBackend::window(0), &running_window()).unwrap();

    let entries = session.dump().await.unwrap();
    let text = dump_text(&entries);

    let listed: Vec<(usize, &str, Role)> = entries
        .iter()
        .map(|e| (e.depth, e.info.name.as_str(), e.info.role))
        .collect();
    assert_eq!(
        listed,
        vec![
            (0, "Settings", Role::Window),
            (1, "Apply", Role::Button),
            (1, "Server address", Role::TextInput),
            (1, "Advanced", Role::Group),
            (2, "Apply", Role::Group),
        ]
    );
    assert!(
        text.contains("  TextInput \"Server address\" [native-TextInput] = \"\""),
        "{text}"
    );
}

#[tokio::test]
async fn a_window_that_exits_is_reported_instead_of_waited_for() {
    let mut session = Session::launch(FakeBackend::window(0), &LaunchSpec::new("true")).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let outcome = session.find("Apply", Role::Button, WAIT).await;

    assert!(matches!(outcome, Err(DriverError::WindowExited(_))));
}

#[tokio::test]
async fn a_program_that_cannot_start_is_a_spawn_error() {
    let outcome = Session::launch(
        FakeBackend::window(0),
        &LaunchSpec::new("/nonexistent/sunnycloud-settings"),
    );

    assert!(matches!(outcome, Err(DriverError::Spawn { .. })));
}
