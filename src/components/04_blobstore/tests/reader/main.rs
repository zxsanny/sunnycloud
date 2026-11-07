//! `BlobReader` (AZ-3728): real decryption of the real files the real writer wrote, rows on a real PostgreSQL 18,
//! tampering done on the files themselves. The `sync-hook` feature only counts chunk reads and the read-ahead
//! window. The 1 GiB row (AC-7) is its own binary, `reader_gib`, because peak RSS is per process.

mod findings;
mod ranges;
mod roundtrip;
mod support;
mod tamper;
