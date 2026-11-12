//! A local HTTP/1.1 host shaped like GitHub Releases (`/<owner>/<repo>/releases/latest/download/<file>` and
//! `/<owner>/<repo>/releases/download/<tag>/<file>`). It stands in for the external release source and
//! records every request path.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::updater::AllowedHost;

pub(crate) const RELEASES: &str = "/zxsanny/sunnycloud/releases";

#[derive(Clone)]
pub(crate) enum Reply {
    Body(Vec<u8>),
    Redirect(String),
}

type Routes = Arc<Mutex<HashMap<String, Reply>>>;

pub(crate) struct ReleaseHost {
    origin: String,
    routes: Routes,
    requests: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl ReleaseHost {
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("addr"));
        let routes: Routes = Arc::default();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let task = tokio::spawn(serve(listener, Arc::clone(&routes), Arc::clone(&requests)));
        Self {
            origin,
            routes,
            requests,
            task,
        }
    }

    /// The release source as the test build is given it.
    pub(crate) fn source(&self) -> String {
        format!("{}{RELEASES}", self.origin)
    }

    /// The host as a redirect rule lists it.
    pub(crate) fn allowed(&self) -> AllowedHost {
        let port = self.origin.rsplit(':').next().and_then(|p| p.parse().ok());
        AllowedHost {
            host: "127.0.0.1".to_owned(),
            port: port.expect("port"),
        }
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }

    /// Answers `path` (an absolute path on this host) with `reply`.
    pub(crate) fn route(&self, path: &str, reply: Reply) {
        self.set(path, Some(reply));
    }

    pub(crate) fn latest_url(&self, file: &str) -> String {
        format!("{}/latest/download/{file}", self.source())
    }

    pub(crate) fn set_latest(&self, file: &str, reply: Option<Reply>) {
        self.set(&format!("{RELEASES}/latest/download/{file}"), reply);
    }

    pub(crate) fn set_asset(&self, tag: &str, file: &str, body: Vec<u8>) {
        self.set(
            &format!("{RELEASES}/download/{tag}/{file}"),
            Some(Reply::Body(body)),
        );
    }

    pub(crate) fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }

    fn set(&self, path: &str, reply: Option<Reply>) {
        let mut routes = self.routes.lock().expect("routes");
        match reply {
            Some(reply) => routes.insert(path.to_owned(), reply),
            None => routes.remove(path),
        };
    }
}

impl Drop for ReleaseHost {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(listener: TcpListener, routes: Routes, requests: Arc<Mutex<Vec<String>>>) {
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(answer(stream, Arc::clone(&routes), Arc::clone(&requests)));
    }
}

async fn answer(mut stream: TcpStream, routes: Routes, requests: Arc<Mutex<Vec<String>>>) {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&head);
    let path = text.split(' ').nth(1).unwrap_or_default().to_owned();
    requests.lock().expect("requests").push(path.clone());
    let reply = routes.lock().expect("routes").get(&path).cloned();
    let (status, extra, body) = match reply {
        Some(Reply::Body(body)) => ("200 OK", String::new(), body),
        Some(Reply::Redirect(to)) => ("302 Found", format!("Location: {to}\r\n"), Vec::new()),
        None => ("404 Not Found", String::new(), Vec::new()),
    };
    let header = format!(
        "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .expect("write head");
    stream.write_all(&body).await.expect("write body");
    stream.shutdown().await.expect("shutdown");
}
