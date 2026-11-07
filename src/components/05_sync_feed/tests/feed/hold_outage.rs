//! AC-8 (IT-05-9): a database outage during a hold. The feed reads through a TCP forwarder that the test can cut:
//! cutting severs the open connections and refuses new ones until it is restored. The shared test server is not
//! paused, since other suites use it at the same time.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use contracts::ChangeFeedError;
use sc_clock::CancellationToken;
use sc_config::Secret;
use sc_db::{Db, DbConfig, DbError, DbRole};
use sqlx::postgres::PgConnectOptions;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};

use crate::hold_support::{Rig, config, cursor, ms, outcome, principal, valid};
use crate::support::Fixture;

struct Forwarder {
    addr: SocketAddr,
    down: Arc<AtomicBool>,
    open: Arc<Mutex<CancellationToken>>,
    accepting: tokio::task::JoinHandle<()>,
}

impl Forwarder {
    async fn start(target: SocketAddr) -> Forwarder {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local address");
        let down = Arc::new(AtomicBool::new(false));
        let open = Arc::new(Mutex::new(CancellationToken::new()));
        let accepting = tokio::spawn({
            let (down, open) = (down.clone(), open.clone());
            async move {
                while let Ok((mut client, _)) = listener.accept().await {
                    if down.load(Ordering::SeqCst) {
                        continue;
                    }
                    let token = open.lock().unwrap_or_else(PoisonError::into_inner).clone();
                    tokio::spawn(async move {
                        let Ok(mut server) = TcpStream::connect(target).await else {
                            return;
                        };
                        tokio::select! {
                            () = token.cancelled() => {}
                            _ = copy_bidirectional(&mut client, &mut server) => {}
                        }
                    });
                }
            }
        });
        Forwarder {
            addr,
            down,
            open,
            accepting,
        }
    }

    fn cut(&self) {
        self.down.store(true, Ordering::SeqCst);
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancel();
    }

    fn restore(&self) {
        *self.open.lock().unwrap_or_else(PoisonError::into_inner) = CancellationToken::new();
        self.down.store(false, Ordering::SeqCst);
    }
}

impl Drop for Forwarder {
    fn drop(&mut self) {
        self.accepting.abort();
    }
}

async fn db_through(fixture: &Fixture, forwarder: &Forwarder) -> Db {
    let config = fixture.db.config();
    let direct: PgConnectOptions = config
        .db_api_url
        .expose()
        .parse()
        .expect("the test URL is a postgres URL");
    let url = config.db_api_url.expose().replacen(
        &format!(":{}/", direct.get_port()),
        &format!(":{}/", forwarder.addr.port()),
        1,
    );
    Db::connect(
        &DbConfig {
            db_api_url: Secret::new(url),
            db_pool_max: 4,
            ..config
        },
        DbRole::Api,
    )
    .await
    .expect("connect through the forwarder")
}

fn target_of(fixture: &Fixture) -> SocketAddr {
    let options: PgConnectOptions = fixture
        .db
        .config()
        .db_api_url
        .expose()
        .parse()
        .expect("the test URL is a postgres URL");
    format!("{}:{}", options.get_host(), options.get_port())
        .parse()
        .expect("the test server is an IP address")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn holds_end_with_a_typed_unavailable_error_at_the_recheck_and_the_same_cursor_reads_the_changes_after_the_resume()
 {
    // Arrange
    let fixture = Fixture::with_pool_max(4).await;
    let forwarder = Forwarder::start(target_of(&fixture)).await;
    let feed_db = db_through(&fixture, &forwarder).await;
    let rig = Rig::over(fixture, feed_db, &config(2_000, 200));
    let account = rig.account().await;
    let held: Vec<_> = (0..3)
        .map(|_| rig.read(&principal(account.id), valid(), cursor(0), ms(30_000)))
        .collect();
    rig.clock.sleeps_reach(3).await;
    forwarder.cut();
    let seq = rig.commit_and_notify(&account).await;

    // Act
    rig.clock.tick(ms(200));
    let mut ended = Vec::new();
    for held in held {
        ended.push(outcome(held).await);
    }
    forwarder.restore();
    let later = rig
        .feed_read_now(&principal(account.id), cursor(0))
        .await
        .expect("page after the resume");

    // Assert
    for result in &ended {
        match result {
            Err(ChangeFeedError::Unexpected(fault)) => {
                assert_eq!(fault.downcast_ref::<DbError>(), Some(&DbError::Unavailable))
            }
            other => panic!("expected the unavailable error, got {other:?}"),
        }
    }
    assert_eq!((later.items.len(), later.next_cursor), (1, cursor(seq)));
    drop(forwarder);
    rig.finish().await;
}
