//! The smallest HTTP/1.1 server plumbing of the in-process provider: one request in, one response out, the
//! connection closed after it.

use std::collections::HashMap;

use reqwest::Url;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, body: &Value) -> Response {
        Response {
            status,
            headers: vec![("Content-Type", "application/json".to_owned())],
            body: body.to_string().into_bytes(),
        }
    }
}

pub async fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    let mut buffer = Vec::new();
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        let mut chunk = [0u8; 2048];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut start = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (
        start.next().unwrap_or_default().to_owned(),
        start.next().unwrap_or_default().to_owned(),
    );
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        let mut chunk = [0u8; 2048];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        body.extend_from_slice(&chunk[..read]);
    }
    let path = target.split('?').next().unwrap_or_default().to_owned();
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

pub fn url_form(body: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(body);
    Url::parse(&format!("http://form.invalid/?{text}"))
        .expect("form body")
        .query_pairs()
        .into_owned()
        .collect()
}

/// Sends `response` and closes the connection; a failed write is reported, there is no one to tell.
pub async fn write_response(stream: &mut TcpStream, response: &Response) {
    let mut head = format!(
        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let written = async {
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(&response.body).await?;
        stream.shutdown().await
    }
    .await;
    if let Err(error) = written {
        eprintln!("fake provider: write failed: {error}");
    }
}
