//! The ffmpeg progress sink (10 section 4, gate S5): `-progress pipe:1` writes blocks of
//! `key=value` lines to the child's stdout, which the guard keeps in a file of `work/`. A block
//! ends at `progress=continue` or `progress=end`. The guard reads the file as it grows; a block
//! is the proof that the child is alive, whatever its values say (`out_time_us` and `speed`
//! read `N/A` at the start).

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use media_shared::decoder_protocol::ranges::{MAX_DURATION_MS, MAX_PROGRESS_SPEED};

/// The bytes read per poll: a child that floods its stdout cannot keep the guard from its other
/// checks.
const READ_PER_POLL: usize = 64 * 1024;
/// A progress line is a few dozen bytes; a longer one is not ffmpeg's and is dropped.
const MAX_LINE: usize = 256;

/// One completed block. `out_time_ms` is `None` while ffmpeg reports `N/A`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Block {
    pub out_time_ms: Option<u64>,
    pub speed: f64,
}

pub(crate) struct ProgressReader {
    file: File,
    line: Vec<u8>,
    overlong: bool,
    out_time_ms: Option<u64>,
    speed: f64,
}

impl ProgressReader {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
            line: Vec::new(),
            overlong: false,
            out_time_ms: None,
            speed: 0.0,
        })
    }

    /// The blocks that ended in what the child wrote since the last call.
    pub(crate) fn poll(&mut self) -> io::Result<Vec<Block>> {
        let mut buffer = [0u8; 4096];
        let mut remaining = READ_PER_POLL;
        let mut blocks = Vec::new();
        while remaining > 0 {
            let want = buffer.len().min(remaining);
            let read = self.file.read(&mut buffer[..want])?;
            if read == 0 {
                break;
            }
            remaining -= read;
            for &byte in &buffer[..read] {
                self.push(byte, &mut blocks);
            }
        }
        Ok(blocks)
    }

    fn push(&mut self, byte: u8, blocks: &mut Vec<Block>) {
        if byte != b'\n' {
            if self.line.len() < MAX_LINE {
                self.line.push(byte);
            } else {
                self.overlong = true;
            }
            return;
        }
        if !self.overlong {
            self.take_line(blocks);
        }
        self.line.clear();
        self.overlong = false;
    }

    fn take_line(&mut self, blocks: &mut Vec<Block>) {
        let Ok(text) = std::str::from_utf8(&self.line) else {
            return;
        };
        let Some((key, value)) = text.trim().split_once('=') else {
            return;
        };
        match key {
            "out_time_us" => {
                self.out_time_ms = value
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .map(|micros| (micros / 1000).min(MAX_DURATION_MS));
            }
            "speed" => self.speed = parse_speed(value),
            "progress" if value == "continue" || value == "end" => {
                blocks.push(Block {
                    out_time_ms: self.out_time_ms.take(),
                    speed: std::mem::replace(&mut self.speed, 0.0),
                });
            }
            _ => {}
        }
    }
}

/// `1.5x`; `N/A` and anything unparseable read as 0.
fn parse_speed(value: &str) -> f64 {
    value
        .trim()
        .trim_end_matches('x')
        .parse::<f64>()
        .ok()
        .filter(|speed| speed.is_finite())
        .map_or(0.0, |speed| speed.clamp(0.0, MAX_PROGRESS_SPEED))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    fn reader_over(text: &str) -> (NamedTempFile, ProgressReader) {
        let mut file = NamedTempFile::new().expect("temp file");
        file.write_all(text.as_bytes()).expect("write");
        let reader = ProgressReader::open(file.path()).expect("open");
        (file, reader)
    }

    #[test]
    fn a_block_ends_at_the_progress_line_and_carries_the_time_and_speed_before_it() {
        let (_file, mut reader) = reader_over(
            "frame=10\nout_time_us=2500000\nspeed=1.5x\nprogress=continue\n\
             out_time_us=5000999\nspeed=2.0x\nprogress=end\n",
        );

        let blocks = reader.poll().expect("poll");

        assert_eq!(
            blocks,
            [
                Block {
                    out_time_ms: Some(2500),
                    speed: 1.5
                },
                Block {
                    out_time_ms: Some(5000),
                    speed: 2.0
                }
            ]
        );
    }

    #[test]
    fn not_available_values_still_complete_a_block_so_the_stall_timer_resets() {
        let (_file, mut reader) = reader_over("out_time_us=N/A\nspeed=N/A\nprogress=continue\n");

        let blocks = reader.poll().expect("poll");

        assert_eq!(
            blocks,
            [Block {
                out_time_ms: None,
                speed: 0.0
            }]
        );
    }

    #[test]
    fn a_line_split_across_two_reads_is_joined_and_an_unfinished_block_is_not_reported() {
        let mut file = NamedTempFile::new().expect("temp file");
        let mut reader = ProgressReader::open(file.path()).expect("open");
        file.write_all(b"out_time_us=1000\nprogr").expect("write");
        let first = reader.poll().expect("poll");
        file.write_all(b"ess=continue\n").expect("write");

        let second = reader.poll().expect("poll");

        assert!(first.is_empty());
        assert_eq!(second.len(), 1);
    }

    #[test]
    fn garbage_overlong_lines_and_unknown_progress_values_are_not_blocks() {
        let flood = "x".repeat(10_000);
        let (_file, mut reader) = reader_over(&format!(
            "{flood}progress=continue\nprogress=maybe\nno equals sign\n\u{0}\u{1}\nspeed=nanx\n"
        ));

        let blocks = reader.poll().expect("poll");

        assert!(blocks.is_empty(), "{blocks:?}");
    }

    #[test]
    fn a_speed_beyond_the_protocol_maximum_is_clamped() {
        assert_eq!(parse_speed("99999x"), MAX_PROGRESS_SPEED);
        assert_eq!(parse_speed("-3x"), 0.0);
        assert_eq!(parse_speed("infx"), 0.0);
    }
}
