mod bounded;
#[cfg(target_os = "linux")]
mod credentials;
#[cfg(target_os = "linux")]
mod fake_api;
#[cfg(target_os = "linux")]
mod file_store;
#[cfg(target_os = "linux")]
mod login;
#[cfg(target_os = "linux")]
mod logout_revoke;

#[cfg(target_os = "linux")]
pub(super) use harness::*;

/// Sessions over the real file store in a temp dir, a `ManualClock` and a recording browser.
#[cfg(target_os = "linux")]
mod harness {
    use std::io;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use ipc_schema::LoginState;
    use sc_clock::{ManualClock, Timestamp};
    use tempfile::TempDir;
    use uuid::Uuid;

    use crate::auth::token_store::{TokenStore, UserFileTokenStore};
    use crate::auth::{BrowserLauncher, LoginTiming, Session, SessionDeps};

    pub(crate) const ACCOUNT: Uuid = Uuid::from_u128(0x0190_d5a2_6b1c_7a00_8000_0000_0000_00a1);
    pub(crate) const DEVICE_TOKEN: &str = "fake-device-token-91d0c4";

    #[derive(Default)]
    pub(crate) struct RecordingBrowser(Mutex<Vec<String>>);

    impl RecordingBrowser {
        pub(crate) fn opened(&self) -> Vec<String> {
            self.0.lock().expect("browser").clone()
        }
    }

    impl BrowserLauncher for RecordingBrowser {
        fn open(&self, url: &str) -> io::Result<()> {
            self.0.lock().expect("browser").push(url.to_owned());
            Ok(())
        }
    }

    pub(crate) struct Harness {
        pub(crate) dir: TempDir,
        pub(crate) clock: Arc<ManualClock>,
        pub(crate) store: Arc<UserFileTokenStore>,
        pub(crate) browser: Arc<RecordingBrowser>,
        pub(crate) session: Session,
    }

    /// The config mock: the RFC 8628 step of 5 s, applied on the manual clock only.
    pub(crate) fn harness() -> Harness {
        let dir = TempDir::new().expect("temp dir");
        let clock = Arc::new(ManualClock::new(Timestamp::from_unix_micros(0)));
        let store = Arc::new(UserFileTokenStore::new(dir.path()));
        let browser = Arc::new(RecordingBrowser::default());
        let session = Session::new(SessionDeps {
            store: Arc::clone(&store) as Arc<dyn TokenStore>,
            account: ACCOUNT,
            browser: Arc::clone(&browser) as Arc<dyn BrowserLauncher>,
            clock: Arc::clone(&clock) as Arc<dyn sc_clock::Clock>,
            timing: LoginTiming {
                slow_down_step: Duration::from_secs(5),
            },
        });
        Harness {
            dir,
            clock,
            store,
            browser,
            session,
        }
    }

    impl Harness {
        /// Moves the manual clock one second at a time until the attempt leaves `Pending`, then waits (by
        /// polling, at most 5 s of real time) for the store write that ends it.
        pub(crate) async fn settle_login(&self, max_seconds: u32) -> LoginState {
            Self::let_the_attempt_run().await;
            for _ in 0..max_seconds {
                if self.session.login_status() != LoginState::Pending {
                    break;
                }
                self.clock.tick(Duration::from_secs(1));
                Self::let_the_attempt_run().await;
            }
            let waited = tokio::time::timeout(Duration::from_secs(5), async {
                while self.session.login_status() == LoginState::Pending {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await;
            assert!(waited.is_ok(), "the login attempt did not settle");
            self.session.login_status()
        }

        /// The attempt runs on this test's single-threaded runtime: yielding lets it reach its next sleep.
        async fn let_the_attempt_run() {
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        }

        pub(crate) fn stored_token(&self) -> Option<String> {
            self.store
                .load(ACCOUNT)
                .expect("load from the file store")
                .map(|token| token.expose().clone())
        }
    }
}
