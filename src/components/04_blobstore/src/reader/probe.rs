//! Test-only counters of the reader. Without the `sync-hook` feature every method is empty.

#[cfg(feature = "sync-hook")]
pub use hooked::ReadProbe;
#[cfg(not(feature = "sync-hook"))]
pub(crate) use idle::ReadProbe;

#[cfg(not(feature = "sync-hook"))]
mod idle {
    #[derive(Default)]
    pub struct ReadProbe;

    impl ReadProbe {
        pub fn chunk_read(&self, _sealed_bytes: usize) {}
        pub fn slot_reserved(&self) {}
        pub fn slot_released(&self) {}
    }
}

#[cfg(feature = "sync-hook")]
mod hooked {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use tokio::sync::watch;

    /// Chunk reads and the read-ahead window of a reader, for the tests of IT-04-3 and the window bound.
    pub struct ReadProbe {
        chunk_reads: AtomicU64,
        chunk_bytes: AtomicU64,
        in_flight: watch::Sender<usize>,
        high_water: AtomicUsize,
    }

    impl Default for ReadProbe {
        fn default() -> Self {
            ReadProbe {
                chunk_reads: AtomicU64::new(0),
                chunk_bytes: AtomicU64::new(0),
                in_flight: watch::channel(0).0,
                high_water: AtomicUsize::new(0),
            }
        }
    }

    impl ReadProbe {
        pub(crate) fn chunk_read(&self, sealed_bytes: usize) {
            self.chunk_reads.fetch_add(1, Ordering::SeqCst);
            self.chunk_bytes
                .fetch_add(sealed_bytes as u64, Ordering::SeqCst);
        }

        pub(crate) fn slot_reserved(&self) {
            let mut now = 0;
            self.in_flight.send_modify(|count| {
                *count += 1;
                now = *count;
            });
            self.high_water.fetch_max(now, Ordering::SeqCst);
        }

        pub(crate) fn slot_released(&self) {
            self.in_flight.send_modify(|count| *count -= 1);
        }

        /// Chunk reads from the file since the reader was built (the 64-byte header is not a chunk).
        pub fn chunk_reads(&self) -> u64 {
            self.chunk_reads.load(Ordering::SeqCst)
        }

        /// Bytes of those reads: 65,552 for each full chunk.
        pub fn chunk_bytes(&self) -> u64 {
            self.chunk_bytes.load(Ordering::SeqCst)
        }

        /// The chunks read ahead of the consumer and not yet taken, now and as a watched value.
        pub fn read_ahead(&self) -> watch::Receiver<usize> {
            self.in_flight.subscribe()
        }

        /// The most chunks that were ever read ahead and waiting at once.
        pub fn high_water(&self) -> usize {
            self.high_water.load(Ordering::SeqCst)
        }
    }
}
