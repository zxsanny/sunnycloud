//! The record of everything FD received: in memory for in-process tests and, when a record file is given, one
//! JSON line per message appended with a single unbuffered write, so a SIGKILL loses no line already received.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use media_shared::decoder_protocol::Request;
use serde::Serialize;
use serde_json::{Map, Value};

/// One received message.
#[derive(Debug, Clone)]
pub struct Record {
    pub at: Instant,
    /// The connection it came on, counted from 1 in accept order.
    pub conn: u64,
    pub request: Request,
    /// Entries of the job's `out/` when a job request arrived; `None` for the other requests.
    pub out_entries: Option<usize>,
}

/// One job request, as the slot loop tests read it.
#[derive(Debug, Clone)]
pub struct Seen {
    pub request: Request,
    /// When the request arrived.
    pub at: Instant,
    /// Entries of the job's `out/` when the request arrived.
    pub out_entries: usize,
}

#[derive(Serialize)]
struct Line<'a> {
    at_ms: u64,
    conn: u64,
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    job: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    out_entries: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
}

pub struct Recorder {
    started: Instant,
    file: Option<Mutex<File>>,
    records: Mutex<Vec<Record>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn job_dir(request: &Request) -> Option<&str> {
    match request {
        Request::Probe { job_dir }
        | Request::RenderStill { job_dir, .. }
        | Request::Transcode { job_dir, .. } => Some(job_dir),
        Request::Hello { .. } | Request::SelfCheck { .. } | Request::Cancel => None,
    }
}

/// The protocol name of the request and its fields: `{"render_still": {...}}` splits into both parts, a bare
/// `"cancel"` has no fields.
fn kind_and_fields(request: &Request) -> (String, Value) {
    match serde_json::to_value(request) {
        Ok(Value::String(kind)) => (kind, Value::Object(Map::new())),
        Ok(Value::Object(map)) if map.len() == 1 => map
            .into_iter()
            .next()
            .unwrap_or_else(|| ("unknown".to_owned(), Value::Null)),
        Ok(other) => ("unknown".to_owned(), other),
        Err(error) => ("unknown".to_owned(), Value::String(error.to_string())),
    }
}

impl Recorder {
    /// Appends to `path` when given; the file is created if missing and never truncated.
    pub fn new(path: Option<&Path>) -> io::Result<Self> {
        let file = path
            .map(|path| OpenOptions::new().create(true).append(true).open(path))
            .transpose()?;
        Ok(Self {
            started: Instant::now(),
            file: file.map(Mutex::new),
            records: Mutex::new(Vec::new()),
        })
    }

    fn ms_since_start(&self, at: Instant) -> u64 {
        u64::try_from(at.duration_since(self.started).as_millis()).unwrap_or(u64::MAX)
    }

    pub(crate) fn request(&self, conn: u64, request: &Request) {
        let at = Instant::now();
        let dir = job_dir(request);
        let out_entries = dir.map(|dir| {
            fs::read_dir(Path::new(dir).join("out")).map_or(0, |entries| entries.count())
        });
        let (kind, fields) = kind_and_fields(request);
        self.write(&Line {
            at_ms: self.ms_since_start(at),
            conn,
            kind: &kind,
            job: dir
                .and_then(|dir| Path::new(dir).file_name())
                .and_then(|name| name.to_str()),
            out_entries,
            fields: Some(fields),
            detail: None,
        });
        lock(&self.records).push(Record {
            at,
            conn,
            request: request.clone(),
            out_entries,
        });
    }

    /// Something FD itself did or failed to do, not a received message (`conn` 0 for the listener).
    pub(crate) fn event(&self, conn: u64, kind: &str, detail: &str) {
        self.write(&Line {
            at_ms: self.ms_since_start(Instant::now()),
            conn,
            kind,
            job: None,
            out_entries: None,
            fields: None,
            detail: Some(detail),
        });
    }

    fn write(&self, line: &Line<'_>) {
        let Some(file) = &self.file else {
            return;
        };
        let mut bytes = match serde_json::to_vec(line) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("fake decoder: cannot encode a record line: {error}");
                return;
            }
        };
        bytes.push(b'\n');
        if let Err(error) = lock(file).write_all(&bytes) {
            eprintln!("fake decoder: cannot append to the record file: {error}");
        }
    }

    pub fn records(&self) -> Vec<Record> {
        lock(&self.records).clone()
    }

    /// The job requests in arrival order.
    pub fn seen(&self) -> Vec<Seen> {
        lock(&self.records)
            .iter()
            .filter_map(|record| {
                record.out_entries.map(|out_entries| Seen {
                    request: record.request.clone(),
                    at: record.at,
                    out_entries,
                })
            })
            .collect()
    }

    pub fn hellos(&self) -> usize {
        lock(&self.records)
            .iter()
            .filter(|record| matches!(record.request, Request::Hello { .. }))
            .count()
    }
}
