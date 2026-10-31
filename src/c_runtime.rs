//! Allocator and panic handler for linking the `no_std` staticlib into
//! C/C++ firmware (feature `c-runtime`).
//!
//! Heap memory comes from the firmware's C library (`malloc`, `free`,
//! `memalign` — newlib on Teensyduino). A panic calls `abort()`; on the
//! device a panic means a programming error (e.g. an unseeded random
//! schedule), so the firmware's own fault handling takes over.

use core::alloc::{GlobalAlloc, Layout};

extern "C" {
    fn malloc(size: usize) -> *mut u8;
    fn memalign(align: usize, size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
    fn abort() -> !;
}

/// Alignment that `malloc` guarantees on 32-bit ARM newlib.
const MALLOC_ALIGN: usize = 8;

struct CAllocator;

// SAFETY: `malloc`/`memalign` return either null or a block of at least
// `size` bytes with the requested alignment, and `free` accepts every
// pointer either of them returned — the contract `GlobalAlloc` requires.
unsafe impl GlobalAlloc for CAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= MALLOC_ALIGN {
            // SAFETY: plain C call; a null return is passed through as
            // allocation failure.
            unsafe { malloc(layout.size()) }
        } else {
            // SAFETY: `layout.align()` is a power of two (guaranteed by
            // `Layout`), which is what `memalign` requires.
            unsafe { memalign(layout.align(), layout.size()) }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // SAFETY: `ptr` came from `alloc` above (the `GlobalAlloc`
        // contract), i.e. from `malloc` or `memalign`.
        unsafe { free(ptr) }
    }
}

#[global_allocator]
static ALLOCATOR: CAllocator = CAllocator;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    // SAFETY: `abort` takes no arguments and never returns.
    unsafe { abort() }
}
