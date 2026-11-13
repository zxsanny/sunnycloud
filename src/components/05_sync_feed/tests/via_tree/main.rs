//! Sync feed through the real 02 `TreeCommit::publish` (AZ-3781, "via 02"): 50 concurrent commits seen once and in
//! order (AC-69), stamp last and notify after COMMIT (AC-1), and a commit on device A waking device B (AC-2,
//! AT-05-1); on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL). Its own test binary, with the tree and
//! quota crates wired in.

mod concurrent;
mod park_clock;
mod stamp_last;
mod support;
mod wake;
