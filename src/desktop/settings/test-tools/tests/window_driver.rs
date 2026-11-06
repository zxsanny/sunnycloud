//! AC-7: one driver script against the real settings window, through the accessibility tree.
//!
//! Ignored by default because it needs what a plain `cargo test` host does not have: the
//! settings window binary (`src/desktop/settings` is still a skeleton whose `main` is empty)
//! and an accessibility session (Linux: a session bus with the AT-SPI bus, under Xvfb or a
//! Wayland session; Windows: an interactive desktop). Run it with
//! `SUNNYCLOUD_SETTINGS_BIN=<path> cargo test -p sunnycloud-settings-testkit --test window_driver -- --ignored --nocapture`.
//! On Windows also set `SUNNYCLOUD_STUB_PIPE` to the pipe name the window is configured to
//! connect to. When a prerequisite is missing the test fails with the reason; it never passes
//! without having driven the window.
//!
//! The control names are those of the desktop-settings mockup
//! (`ui_mockups/desktop-settings.html`); the window must expose them as accessible names.

mod common;

use std::time::Duration;

use sunnycloud_settings_testkit::driver::{LaunchSpec, NativeBackend, Role, Session, dump_text};
use sunnycloud_settings_testkit::stub::StubDaemon;

use common::{script_with, signed_in};

const FIND: Duration = Duration::from_secs(10);

fn window_binary() -> String {
    std::env::var("SUNNYCLOUD_SETTINGS_BIN").unwrap_or_else(|_| {
        panic!("SUNNYCLOUD_SETTINGS_BIN is not set: AC-7 needs the settings window binary")
    })
}

async fn start_stub() -> StubDaemon {
    let script = script_with(signed_in());
    #[cfg(unix)]
    let stub = StubDaemon::start(script).await;
    #[cfg(windows)]
    let stub = {
        let pipe = std::env::var("SUNNYCLOUD_STUB_PIPE")
            .expect("SUNNYCLOUD_STUB_PIPE names the pipe the window connects to");
        StubDaemon::start_on_pipe(script, pipe).await
    };
    stub.expect("stub starts")
}

#[tokio::test]
#[ignore = "AC-7 needs the settings window binary (a skeleton today) and an AT-SPI / UI Automation session"]
async fn the_driver_acts_on_the_real_window_through_the_accessibility_tree() {
    let stub = start_stub().await;
    let backend = NativeBackend::connect()
        .await
        .expect("the accessibility service is reachable");
    let spec = LaunchSpec::new(window_binary());
    #[cfg(unix)]
    let spec = spec.env("XDG_RUNTIME_DIR", stub.runtime_dir());
    let mut session = Session::launch(backend, &spec).expect("the window starts");

    let first = session
        .first_interactive(FIND)
        .await
        .expect("the window shows an interactive control");
    println!("first interactive control after {first:?}");
    let add = session
        .find("Add folder pair", Role::Button, FIND)
        .await
        .expect("find a control by accessible name and role");
    session.click(&add).await.expect("click it");
    let path = session
        .find("Server folder path", Role::TextInput, FIND)
        .await
        .expect("find the text field");
    session
        .type_text(&path, "Photos/2026")
        .await
        .expect("type into it");
    let value = session.value(&path).await.expect("read its value");
    let entries = session.dump().await.expect("dump the tree");
    println!("{}", dump_text(&entries));

    assert_eq!(value.as_deref(), Some("Photos/2026"));
    assert!(
        entries
            .iter()
            .any(|e| e.info.name == "Add folder pair" && e.info.role == Role::Button)
    );
    assert!(entries.iter().all(|e| !e.info.native_role.is_empty()));
    assert!(
        stub.count("status") >= 1,
        "the window never asked the stub for its status"
    );
    session.close().await.expect("stop the window");
    stub.shutdown().await.expect("stub shuts down");
}
