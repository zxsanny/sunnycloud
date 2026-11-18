//! A blocking HTTP/1.1 client over a plain socket (the API image has no client either): one request per
//! connection, `Connection: close`, body read to the end, response headers kept (names lowercased).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::Value;

const HOST: &str = "cloud.example.org";
const IO_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    /// Header names lowercased, values trimmed, in the order received.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(found, _)| found.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "status {}, body is not JSON ({error}): {}",
                self.status,
                String::from_utf8_lossy(&self.body)
            )
        })
    }

    /// The `code` field of a JSON error body, empty when it has none.
    pub fn code(&self) -> String {
        self.json()["code"].as_str().unwrap_or_default().to_owned()
    }
}

/// The status of `GET path`, or `None` when nothing answers.
pub fn get_status(addr: SocketAddr, path: &str) -> Option<u16> {
    exchange(addr, "GET", path, &[], &[])
        .ok()
        .map(|answer| answer.status)
}

/// `method path` with a bearer token, extra headers and a body; panics on a transport failure.
pub fn call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> Response {
    let mut all = vec![("Authorization", format!("Bearer {token}"))];
    all.extend(headers.iter().cloned());
    send(addr, method, path, &all, body)
}

/// `method path` with exactly `headers` (no implied credentials) and a body; panics on a transport failure.
pub fn send(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> Response {
    exchange(addr, method, path, headers, body)
        .unwrap_or_else(|error| panic!("{method} {path} on {addr}: {error}"))
}

/// `sunnycloud_janitor_cycles_total{task, result="ok"}` of the ops listener's `/metrics`.
pub fn janitor_cycles(ops: SocketAddr, task: &str) -> u64 {
    let scrape = send(ops, "GET", "/metrics", &[], b"");
    let text = String::from_utf8_lossy(&scrape.body);
    let label = format!("task=\"{task}\"");
    text.lines()
        .filter(|line| line.starts_with("sunnycloud_janitor_cycles_total{"))
        .filter(|line| line.contains(&label) && line.contains(r#"result="ok""#))
        .filter_map(|line| line.rsplit(' ').next()?.parse::<f64>().ok())
        .map(|count| count as u64)
        .sum()
}

fn exchange(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> std::io::Result<Response> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {HOST}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse(&raw)
}

fn parse(raw: &[u8]) -> std::io::Result<Response> {
    let invalid =
        |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_owned());
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| invalid("no end of the response head"))?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| invalid("head is not UTF-8"))?;
    let body = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| invalid("no status line"))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let chunked = headers
        .iter()
        .any(|(name, value)| name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked"));
    let body = if chunked {
        dechunk(body).ok_or_else(|| invalid("bad chunked body"))?
    } else {
        body.to_vec()
    };
    Ok(Response {
        status,
        headers,
        body,
    })
}

fn dechunk(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let end = rest.windows(2).position(|window| window == b"\r\n")?;
        let size_text = std::str::from_utf8(&rest[..end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        rest = &rest[end + 2..];
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(rest.get(..size)?);
        rest = rest.get(size + 2..)?;
    }
}
