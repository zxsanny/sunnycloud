use ipc_schema::LineBuffer;

/// The window rejects a frame longer than this. It equals the daemon's own cap
/// (`ipc_schema.md` open point 8 leaves the number open): the largest legitimate frame is a
/// `DaemonStatus` or a listing page, far below 1 MiB.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct FrameTooLong;

/// `LineBuffer` with a cap on the line being received. The check runs before the bytes are
/// stored, so the buffer never holds more than the cap plus one chunk; the caller drops the
/// reader after an error and the rejected bytes go with it.
#[derive(Debug)]
pub struct FrameReader {
    lines: LineBuffer,
    /// Bytes of the line still being received, without its `\n`.
    partial: usize,
    max: usize,
}

impl FrameReader {
    pub fn new(max: usize) -> Self {
        Self {
            lines: LineBuffer::default(),
            partial: 0,
            max,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), FrameTooLong> {
        let mut partial = self.partial;
        for segment in chunk.split_inclusive(|b| *b == b'\n') {
            let complete = segment.ends_with(b"\n");
            partial += segment.len() - usize::from(complete);
            if partial > self.max {
                return Err(FrameTooLong);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(reader: &mut FrameReader) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| reader.next_line()).collect()
    }

    #[test]
    fn a_line_split_over_chunks_is_returned_once_complete() {
        let mut reader = FrameReader::new(64);

        reader.push(b"{\"id\":").unwrap();
        let before = drain(&mut reader);
        reader.push(b"1}\n{\"id\"").unwrap();
        let first = drain(&mut reader);
        reader.push(b":2}\n").unwrap();
        let second = drain(&mut reader);

        assert!(before.is_empty());
        assert_eq!(first, vec![b"{\"id\":1}".to_vec()]);
        assert_eq!(second, vec![b"{\"id\":2}".to_vec()]);
    }

    #[test]
    fn several_lines_in_one_chunk_come_out_in_order() {
        let mut reader = FrameReader::new(64);

        reader.push(b"a\nb\n\nc").unwrap();

        assert_eq!(
            drain(&mut reader),
            vec![b"a".to_vec(), b"b".to_vec(), vec![]]
        );
    }

    #[test]
    fn a_line_of_exactly_the_cap_is_accepted_and_one_more_byte_is_not() {
        let mut ok = FrameReader::new(4);
        let mut over = FrameReader::new(4);

        let accepted = ok.push(b"abcd\n");
        let rejected = over.push(b"abcde");

        assert_eq!(accepted, Ok(()));
        assert_eq!(rejected, Err(FrameTooLong));
    }

    #[test]
    fn an_oversize_frame_is_rejected_before_more_than_the_cap_plus_a_chunk_is_stored() {
        let mut reader = FrameReader::new(MAX_FRAME_BYTES);
        let chunk = vec![b'x'; 64 * 1024];
        let mut accepted = 0;

        let rejected = loop {
            match reader.push(&chunk) {
                Ok(()) => accepted += chunk.len(),
                Err(error) => break error,
            }
        };

        assert_eq!(rejected, FrameTooLong);
        assert!(accepted <= MAX_FRAME_BYTES);
    }

    #[test]
    fn the_cap_counts_each_line_not_the_stream() {
        let mut reader = FrameReader::new(4);

        let result = (0..100).try_for_each(|_| reader.push(b"abcd\n"));

        assert_eq!(result, Ok(()));
    }
}
