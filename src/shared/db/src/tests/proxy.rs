//! A TCP forwarder in front of the test database. Cutting it closes the listener and every open connection
//! (the database "stops" for the pool); restoring it listens on the same port again.

use std::sync::Mutex;

use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

pub struct Proxy {
    port: u16,
    target: (String, u16),
    accept_loop: Mutex<Option<JoinHandle<()>>>,
}

impl Proxy {
    pub async fn start(target_host: &str, target_port: u16) -> Proxy {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind proxy");
        let port = listener.local_addr().expect("proxy address").port();
        let proxy = Proxy {
            port,
            target: (target_host.to_string(), target_port),
            accept_loop: Mutex::new(None),
        };
        proxy.spawn(listener);
        proxy
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Refuses new connections and closes the open ones.
    pub fn cut(&self) {
        let handle = self.accept_loop.lock().expect("proxy lock").take();
        if let Some(handle) = handle {
            handle.abort();
        }
    }

    pub async fn restore(&self) {
        let listener = TcpListener::bind(("127.0.0.1", self.port))
            .await
            .expect("rebind proxy port");
        self.spawn(listener);
    }

    fn spawn(&self, listener: TcpListener) {
        let target = self.target.clone();
        let handle = tokio::spawn(accept_loop(listener, target));
        *self.accept_loop.lock().expect("proxy lock") = Some(handle);
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.cut();
    }
}

// Aborting this task drops the listener and the JoinSet, which aborts every forwarding task and closes its sockets.
async fn accept_loop(listener: TcpListener, target: (String, u16)) {
    let mut connections = JoinSet::new();
    loop {
        let (mut client, _) = listener.accept().await.expect("proxy accept");
        let target = target.clone();
        connections.spawn(async move {
            match TcpStream::connect((target.0.as_str(), target.1)).await {
                Ok(mut upstream) => {
                    if let Err(error) = copy_bidirectional(&mut client, &mut upstream).await {
                        eprintln!("proxy: connection ended: {error}");
                    }
                }
                Err(error) => eprintln!("proxy: cannot reach the database: {error}"),
            }
        });
    }
}
