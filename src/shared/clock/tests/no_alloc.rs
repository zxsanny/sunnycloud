//! NFR: `now_utc` and `monotonic` do not allocate. The counter is per thread, so an allocation by another thread of
//! the process (the test harness, the coverage runtime) is not counted against the clock.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::time::Duration;

use sc_clock::{Clock, ManualClock, SystemClock, Timestamp};

struct CountingAllocator;

thread_local! {
    // A const initialiser without a destructor: reading it from inside the allocator does not allocate.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count_allocation() {
    ALLOCATIONS.with(|count| count.set(count.get() + 1));
}

// SAFETY: every call is forwarded unchanged to the system allocator; only a counter is added.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn allocations_during<T>(read: impl Fn() -> T) -> usize {
    let before = ALLOCATIONS.with(Cell::get);
    for _ in 0..1_000 {
        black_box(read());
    }
    ALLOCATIONS.with(Cell::get) - before
}

#[test]
fn clock_reads_do_not_allocate() {
    let system = SystemClock::new();
    let manual = ManualClock::new(Timestamp::from_unix_micros(0));
    manual.tick(Duration::from_secs(1));

    assert_eq!(0, allocations_during(|| system.now_utc()));
    assert_eq!(0, allocations_during(|| system.monotonic()));
    assert_eq!(0, allocations_during(|| manual.now_utc()));
    assert_eq!(0, allocations_during(|| manual.monotonic()));

    #[cfg(feature = "test-clock")]
    {
        let offset = sc_clock::OffsetClock::new();
        offset.advance(Duration::from_secs(1)).expect("advance");
        assert_eq!(0, allocations_during(|| offset.now_utc()));
        assert_eq!(0, allocations_during(|| offset.monotonic()));
    }
}
