use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
};

pub struct CountingAllocator;
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(size: usize) {
    CALLS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size, Relaxed);
    let live = LIVE.fetch_add(size, Relaxed) + size;
    PEAK.fetch_max(live, Relaxed);
}

// Instrument only this example. Delegating each operation to System preserves
// alignment and allocation contracts; atomics never allocate recursively.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            LIVE.fetch_sub(layout.size(), Relaxed);
            allocated(size);
        }
        result
    }
}

pub struct Sample {
    calls: usize,
    bytes: usize,
    live: usize,
}

impl Sample {
    pub fn begin() -> Self {
        let live = LIVE.load(Relaxed);
        PEAK.store(live, Relaxed);
        Self {
            calls: CALLS.load(Relaxed),
            bytes: BYTES.load(Relaxed),
            live,
        }
    }

    pub fn finish(self) -> (usize, usize, usize) {
        (
            CALLS.load(Relaxed) - self.calls,
            BYTES.load(Relaxed) - self.bytes,
            PEAK.load(Relaxed).saturating_sub(self.live),
        )
    }
}
