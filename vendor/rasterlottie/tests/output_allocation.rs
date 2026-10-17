#![cfg(not(target_arch = "wasm32"))]
#![forbid(unsafe_op_in_unsafe_fn)]

use rasterlottie::{Animation, RenderConfig, Renderer, Rgba8};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Counts {
    allocations: usize,
    frees: usize,
    reallocations: usize,
    requested: usize,
    live: isize,
    peak: isize,
}
thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<Counts> = const { Cell::new(Counts {
        allocations: 0, frees: 0, reallocations: 0, requested: 0, live: 0, peak: 0,
    }) };
}
struct Tracking;
fn record(allocate: usize, release: usize, realloc: bool) {
    if ACTIVE.try_with(Cell::get).unwrap_or(false) {
        let _ = COUNTS.try_with(|slot| {
            let mut count = slot.get();
            count.allocations += usize::from(allocate > 0);
            count.frees += usize::from(release > 0);
            count.reallocations += usize::from(realloc);
            count.requested += allocate;
            count.live += allocate as isize - release as isize;
            count.peak = count.peak.max(count.live);
            slot.set(count);
        });
    }
}
// Delegate allocation and layout ownership unchanged to System. Tracking uses
// only const-initialized thread-local Cells and does not allocate or hold locks.
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            record(layout.size(), 0, false);
        }
        result
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            record(layout.size(), 0, false);
        }
        result
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        record(0, layout.size(), false);
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, old, size) };
        if !result.is_null() {
            record(size, old.size(), true);
        }
        result
    }
}
#[global_allocator]
static ALLOCATOR: Tracking = Tracking;

struct Watch;
impl Watch {
    fn start() -> Self {
        assert!(!ACTIVE.with(Cell::get), "watch must not be nested");
        COUNTS.with(|counts| counts.set(Counts::default()));
        ACTIVE.with(|active| active.set(true));
        Self
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        // Also disable tracking if rendering unwinds, before the test reports it.
        ACTIVE.with(|active| active.set(false));
    }
}

#[test]
fn public_prepared_frame_transfers_canvas_without_duplicate_allocation() {
    let animation = Animation::from_json_str(
        r#"{"v":"5.7.6","fr":30,"ip":0,"op":10,"w":16,"h":8,"layers":[]}"#,
    )
    .unwrap();
    let renderer = Renderer::target_corpus();
    let prepared = renderer.prepare(&animation).unwrap();
    for (background, expected) in [
        (Rgba8::TRANSPARENT, [0, 0, 0, 0]),
        (Rgba8::new(0x33, 0x66, 0x99, 255), [0x33, 0x66, 0x99, 255]),
        (Rgba8::new(255, 0, 0, 128), [128, 0, 0, 128]),
    ] {
        let config = RenderConfig::new(background, 1.0);
        // Preparation and warmup are outside the requested-payload watch.
        drop(prepared.render_frame(0.0, config).unwrap());
        let watch = Watch::start();
        let frame = prepared.render_frame(0.0, config).unwrap();
        let pixels_match = frame.width == 16
            && frame.height == 8
            && frame.pixels.len() == 512
            && frame.pixels.chunks_exact(4).all(|pixel| pixel == expected);
        let retained = COUNTS.with(Cell::get);
        drop(frame);
        let dropped = COUNTS.with(Cell::get);
        drop(watch);
        // All oracles/reporting run with tracking disabled. Returned bytes are
        // still premultiplied RGBA; this is not allocator overhead or RSS.
        assert!(pixels_match);
        assert_eq!(
            retained,
            Counts {
                allocations: 1,
                frees: 0,
                reallocations: 0,
                requested: 512,
                live: 512,
                peak: 512,
            }
        );
        assert_eq!(
            dropped,
            Counts {
                frees: 1,
                live: 0,
                ..retained
            }
        );
    }
}
