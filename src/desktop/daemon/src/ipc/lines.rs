use ipc_schema::LineBuffer;

/// A request line longer than the cap is not a request: the longest legitimate one is an
/// `add_pair` or `update_pair` carrying a rules text, which is a few KiB. 1 MiB leaves two
/// orders of magnitude of headroom and bounds the memory one connection can pin.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct LineTooLong;

/// `LineBuffer` with the maximum line length that the contract leaves open (open point 8).
/// The check runs before the bytes are stored, so the buffer never exceeds the cap plus
/// one chunk.
#[derive(Debug)]
pub struct CappedLines {
    lines: LineBuffer,
    /// Bytes of the line still being received, without its `\n`.
    partial: usize,
    max: usize,
}

impl CappedLines {
    pub fn new(max: usize) -> Self {
        Self {
            lines: LineBuffer::default(),
            partial: 0,
            max,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), LineTooLong> {
        let mut partial = self.partial;
        for segment in chunk.split_inclusive(|b| *b == b'\n') {
            let complete = segment.ends_with(b"\n");
            partial += segment.len() - usize::from(complete);
            if partial > self.max {
                return Err(LineTooLong);
            }
            if complete {
                partial = 0;
            }
        }
        self.partial = partial;
        self.lines.push(chunk);
        Ok(())
    }

    pub fn next_line(&mut self) -> Option<Vec<u8>> {
        self.lines.next_line()
    }
}
