//! The script: what FD answers to `hello`, `self_check` and each kind of job request. The file format is in the
//! crate documentation; [`Script::default`] is the script of an in-process fake that no test has scripted yet.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use media_shared::decoder_protocol::{
    Capabilities, DecodeError, ImageResult, Message, ProtocolViolation, Response, SelfCheckItem,
    SelfCheckReport, TranscodeResult, encode_message,
};
use media_shared::policy::Probe;
use serde::Deserialize;

use crate::forms::{garbage, millis, optional_millis, partial_hello};

/// The capabilities `hello` reports unless the script says otherwise.
pub fn capabilities() -> Capabilities {
    Capabilities {
        protocol: 2,
        vips: "8.15".into(),
        ffmpeg: "7.1".into(),
        libheif: "1.19".into(),
        build_timestamp: 1_700_000_000,
        nvenc_probe_ok: false,
        nvenc_sessions: 0,
    }
}

/// What FD does with one job request.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Answer {
    Probe(Probe),
    /// Puts `output` at `out/result.webp`, then answers `image_result` with these dimensions.
    Image {
        width: u32,
        height: u32,
        #[serde(default)]
        output: Output,
    },
    /// Puts `output` at `out/result.mp4`, then answers `transcode_result` with this probe of the output.
    Transcode {
        probe_of_output: Probe,
        #[serde(default)]
        output: Output,
    },
    Error(DecodeError),
    /// Reads the request and closes the connection, as a runner that exits mid-job does.
    Hang,
    /// A hung runner: reads the request, sends no frame of any kind and ignores `cancel`.
    Silent,
    /// Sends `Alive` at this interval but never answers the request and ignores `cancel`.
    AliveOnly {
        #[serde(rename = "every_ms", deserialize_with = "millis")]
        every: Duration,
    },
    /// Writes these bytes instead of an answer, as a compromised runner does, then reads and ignores everything.
    Garbage(#[serde(deserialize_with = "garbage")] Vec<u8>),
    /// Plays `answer` only after `delay`, sending `Alive` every `alive_every` meanwhile when set; a `cancel`
    /// during the delay is answered `cancelled`.
    OverTime {
        #[serde(rename = "delay_ms", deserialize_with = "millis")]
        delay: Duration,
        #[serde(
            rename = "alive_every_ms",
            default,
            deserialize_with = "optional_millis"
        )]
        alive_every: Option<Duration>,
        answer: Box<Answer>,
    },
    /// Connection level: closes every connection and the listener (connects are refused, the socket file stays,
    /// as a dead runner leaves it), waits `refuse`, then binds the socket again.
    Outage {
        #[serde(rename = "refuse_ms", deserialize_with = "millis")]
        refuse: Duration,
    },
}

impl Answer {
    /// A render answer that writes a regular `out/result.webp`.
    pub fn image(width: u32, height: u32) -> Self {
        Answer::Image {
            width,
            height,
            output: Output::File,
        }
    }

    /// A transcode answer that writes a regular `out/result.mp4`.
    pub fn transcode(probe_of_output: Probe) -> Self {
        Answer::Transcode {
            probe_of_output,
            output: Output::File,
        }
    }
}

/// What an `image` or `transcode` answer leaves at `out/result.<ext>`. Every kind but `file` is hostile output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Output {
    /// A regular file of a few fake bytes (not media; the API's sniffer refuses them).
    #[default]
    File,
    /// A regular file of exactly these bytes, in the chunk form of `garbage`: for a control case whose output the
    /// API's sniffer must accept.
    Bytes(#[serde(deserialize_with = "garbage")] Vec<u8>),
    /// Nothing: the answer claims a file that is not there.
    Absent,
    Symlink {
        target: PathBuf,
    },
    /// A hard link to the file of this name in the job's `in/`, so the output has `st_nlink` 2.
    HardLink {
        in_file: String,
    },
    Fifo,
    /// A sparse file of this apparent size.
    Sparse {
        bytes: u64,
    },
    Directory,
    /// A regular file of these bytes (chunk form) at the result name and a symlink to `target` beside it; once the
    /// answer is sent, FD exchanges the two names `times` times, so a reader racing the name meets either (ST-09-3).
    Swap {
        target: PathBuf,
        #[serde(deserialize_with = "garbage")]
        bytes: Vec<u8>,
        times: u32,
    },
}

/// The most exchanges one `swap` output may ask for.
pub const MAX_SWAPS: u32 = 1_000_000;

impl Output {
    /// The `bytes` the answer reports when the size is not known before the file is planted.
    pub(crate) fn reported_bytes(&self) -> u64 {
        match self {
            Output::Sparse { bytes } => (*bytes).max(1),
            Output::Bytes(bytes) | Output::Swap { bytes, .. } => {
                u64::try_from(bytes.len()).unwrap_or(u64::MAX).max(1)
            }
            _ => 1,
        }
    }
}

/// The answers of one kind of request: the queue first, then `default` for every request after it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Queue {
    #[serde(default)]
    pub queue: VecDeque<Answer>,
    pub default: Answer,
}

impl Queue {
    pub fn new(default: Answer) -> Self {
        Self {
            queue: VecDeque::new(),
            default,
        }
    }

    /// The next scripted answer, or the default once the queue is empty.
    pub fn next_answer(&mut self) -> Answer {
        self.queue
            .pop_front()
            .unwrap_or_else(|| self.default.clone())
    }
}

/// Once, `after` the first `self_check` answer, FD goes away as [`Answer::Outage`] does: the supervisor is idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdleOutage {
    #[serde(rename = "after_ms", deserialize_with = "millis")]
    pub after: Duration,
    #[serde(rename = "refuse_ms", deserialize_with = "millis")]
    pub refuse: Duration,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Script {
    #[serde(deserialize_with = "partial_hello")]
    pub hello: Capabilities,
    pub self_check: SelfCheckReport,
    pub probe: Queue,
    pub render_still: Queue,
    pub transcode: Queue,
    pub idle_outage: Option<IdleOutage>,
}

impl Default for Script {
    /// `hello` with [`capabilities`], one passing `no_network` item, a probe answers `unsupported` and every
    /// render and transcode `corrupt`.
    fn default() -> Self {
        Self {
            hello: capabilities(),
            self_check: SelfCheckReport(vec![SelfCheckItem {
                name: "no_network".into(),
                ok: true,
            }]),
            probe: Queue::new(Answer::Error(DecodeError::Unsupported)),
            render_still: Queue::new(Answer::Error(DecodeError::Corrupt)),
            transcode: Queue::new(Answer::Error(DecodeError::Corrupt)),
            idle_outage: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("cannot read the script {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the script is not valid: {0}")]
    Json(#[from] serde_json::Error),
    /// FD holds itself to the protocol's range table, as `write_message` does; out-of-range bytes are `garbage`.
    #[error("the script holds an answer FD may not send: {0}")]
    Unsendable(ProtocolViolation),
    #[error("{0} must be at least 1 ms")]
    ZeroInterval(&'static str),
    #[error("hard_link.in_file must be a plain file name, not {0:?}")]
    NotAFileName(String),
    #[error("swap.times must be 1 to {MAX_SWAPS}, not {0}")]
    SwapTimes(u32),
}

impl Script {
    pub fn parse(text: &str) -> Result<Self, ScriptError> {
        let script: Script = serde_json::from_str(text)?;
        script.check()?;
        Ok(script)
    }

    pub fn read(path: &Path) -> Result<Self, ScriptError> {
        let text = std::fs::read_to_string(path).map_err(|source| ScriptError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text)
    }

    fn check(&self) -> Result<(), ScriptError> {
        sendable(Response::Capabilities(self.hello.clone()))?;
        sendable(Response::SelfCheck(self.self_check.clone()))?;
        for queue in [&self.probe, &self.render_still, &self.transcode] {
            for answer in queue.queue.iter().chain([&queue.default]) {
                check_answer(answer)?;
            }
        }
        Ok(())
    }
}

fn sendable(response: Response) -> Result<(), ScriptError> {
    encode_message(&Message::Response(response))
        .map(drop)
        .map_err(ScriptError::Unsendable)
}

fn at_least_one_ms(interval: Duration, field: &'static str) -> Result<(), ScriptError> {
    if interval < Duration::from_millis(1) {
        return Err(ScriptError::ZeroInterval(field));
    }
    Ok(())
}

fn check_output(output: &Output) -> Result<(), ScriptError> {
    if let Output::HardLink { in_file } = output
        && (in_file.is_empty() || in_file.contains('/') || in_file == "." || in_file == "..")
    {
        return Err(ScriptError::NotAFileName(in_file.clone()));
    }
    if let Output::Swap { times, .. } = output
        && !(1..=MAX_SWAPS).contains(times)
    {
        return Err(ScriptError::SwapTimes(*times));
    }
    Ok(())
}

fn check_answer(answer: &Answer) -> Result<(), ScriptError> {
    match answer {
        Answer::Probe(probe) => sendable(Response::Probe(probe.clone())),
        Answer::Image {
            width,
            height,
            output,
        } => {
            check_output(output)?;
            sendable(Response::ImageResult(ImageResult {
                width: *width,
                height: *height,
                bytes: output.reported_bytes(),
            }))
        }
        Answer::Transcode {
            probe_of_output,
            output,
        } => {
            check_output(output)?;
            sendable(Response::TranscodeResult(TranscodeResult {
                probe_of_output: probe_of_output.clone(),
                bytes: output.reported_bytes(),
            }))
        }
        Answer::Error(error) => sendable(Response::Error(*error)),
        Answer::AliveOnly { every } => at_least_one_ms(*every, "alive_only.every_ms"),
        Answer::OverTime {
            alive_every,
            answer,
            ..
        } => {
            if let Some(every) = alive_every {
                at_least_one_ms(*every, "over_time.alive_every_ms")?;
            }
            check_answer(answer)
        }
        Answer::Hang | Answer::Silent | Answer::Garbage(_) | Answer::Outage { .. } => Ok(()),
    }
}
