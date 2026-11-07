//! Scrubber (AZ-3745): `BlobScrubber` and `ScrubJanitor` against a real PostgreSQL 18, real encrypted files written
//! by the real writer, and the real GC sweeper. Time is the system clock, because the rate cap sleeps on it; rates
//! and budgets are test-sized. Interleavings with the collector are driven by the scrubber's and the sweeper's
//! test hooks (feature `sync-hook`).

mod cycle;
mod pause;
mod races;
mod rate;
mod support;
mod termination;
