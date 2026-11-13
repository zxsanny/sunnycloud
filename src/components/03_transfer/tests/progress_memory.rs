//! AC-6: 100 sessions declaring 50 GiB each (25,600 blocks) hold at most 80 MiB in the progress table. The
//! bytes are measured at the allocator, not computed: a counting global allocator reports what the table
//! really holds after the entries are built. One test only, so no other thread allocates meanwhile.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use contracts::{BlobHandle, BlobPlan};
use sc_clock::{Clock, ManualClock, Timestamp};
use sunnycloud_transfer::{BLOCK_SIZE, CheckpointMark, ProgressTable, SessionInfo};
use uuid::Uuid;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded unchanged to the system allocator; the counter only observes sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::SeqCst);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::SeqCst);
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const SESSIONS: usize = 100;
const DECLARED: u64 = 50 * 1024 * 1024 * 1024;
const LIMIT: usize = 80 * 1024 * 1024;
const FAIL_ABOVE: usize = 100_000_000;

#[test]
fn a_hundred_sessions_of_fifty_gib_fit_in_eighty_mebibytes() {
    // Arrange
    let blocks = u32::try_from(DECLARED / BLOCK_SIZE as u64).expect("block count");
    let hash_bytes = SESSIONS * blocks as usize * 32;
    let before = LIVE.load(Ordering::SeqCst);
    let clock = ManualClock::new(Timestamp::from_unix_micros(0));
    let mark = CheckpointMark {
        at: clock.monotonic(),
        expires_at: clock.now_utc(),
    };
    let declared = i64::try_from(DECLARED).expect("declared size");
    let table = ProgressTable::new();

    // Act
    for _ in 0..SESSIONS {
        let info = SessionInfo {
            account: Uuid::new_v4(),
            declared_size: declared,
            handle: BlobHandle {
                plan: BlobPlan {
                    blob_id: Uuid::new_v4(),
                    wrapped_key: vec![7; 72],
                    master_key_id: 1,
                },
                plaintext_size: declared,
            },
        };
        table.insert_new(Uuid::new_v4(), info, blocks, mark);
    }
    let measured = LIVE.load(Ordering::SeqCst) - before;
    let computed = table.memory_bytes();

    // Assert
    assert_eq!(blocks, 25_600);
    assert_eq!(table.len(), SESSIONS);
    assert!(
        measured >= hash_bytes,
        "{measured} bytes cannot hold {hash_bytes} bytes of hashes"
    );
    assert!(
        measured >= computed,
        "measured {measured} below computed {computed}"
    );
    assert!(
        measured < FAIL_ABOVE,
        "measured {measured} bytes, hard limit {FAIL_ABOVE}"
    );
    assert!(
        measured <= LIMIT,
        "measured {measured} bytes, limit {LIMIT}"
    );
}
