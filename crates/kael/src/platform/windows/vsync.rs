use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use util::ResultExt;
use windows::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{DWM_TIMING_INFO, DwmFlush, DwmGetCompositionTimingInfo},
    System::Performance::QueryPerformanceFrequency,
};

static QPC_TICKS_PER_SECOND: LazyLock<u64> = LazyLock::new(|| {
    let mut frequency = 0;
    if unsafe { QueryPerformanceFrequency(&mut frequency) }.is_err() || frequency <= 0 {
        // Modern Windows documents this as infallible, but a defensive fallback keeps
        // malformed timing state from turning a presentation thread into a process crash.
        log::error!("QueryPerformanceFrequency failed; using the documented 10 MHz baseline");
        10_000_000
    } else {
        frequency as u64
    }
});

const VSYNC_INTERVAL_THRESHOLD: Duration = Duration::from_millis(1);
const DEFAULT_VSYNC_INTERVAL: Duration = Duration::from_micros(16_666); // ~60Hz
/// Re-query the DWM interval every N frames to detect monitor refresh rate changes
/// (e.g. a window dragged from a 60Hz to a 144Hz display, or a mode switch). The
/// interval is also re-queried on every fallback (slept) frame, so a stale interval
/// self-corrects within at most one cadence window.
///
/// The wait itself is already event-driven: `DwmFlush` blocks on the compositor's
/// vblank rather than spinning. `DCompositionWaitForCompositorClock` was evaluated as
/// an alternative but would require restructuring the vsync thread (it waits on a
/// composition-clock handle with its own cancellation semantics) for no pacing benefit
/// over `DwmFlush`, so it is intentionally not used here.
const INTERVAL_REFRESH_CADENCE: u32 = 120;

pub(crate) struct VSyncProvider {
    interval: Duration,
    f: Box<dyn Fn() -> bool>,
    frames_since_refresh: u32,
}

impl VSyncProvider {
    pub(crate) fn new() -> Self {
        let interval = query_dwm_interval();
        let f = Box::new(|| unsafe { DwmFlush().is_ok() });
        Self {
            interval,
            f,
            frames_since_refresh: 0,
        }
    }

    pub(crate) fn wait_for_vsync(&mut self) {
        // Periodically re-query the DWM interval to adapt to monitor switches
        // (e.g., window moved from 60Hz to 144Hz display).
        self.frames_since_refresh += 1;
        if self.frames_since_refresh >= INTERVAL_REFRESH_CADENCE {
            self.frames_since_refresh = 0;
            self.interval = query_dwm_interval();
        }

        let vsync_start = Instant::now();
        let wait_succeeded = (self.f)();
        let elapsed = vsync_start.elapsed();
        // DwmFlush and DCompositionWaitForCompositorClock returns very early
        // instead of waiting until vblank when the monitor goes to sleep or is
        // unplugged (nothing to present due to desktop occlusion). We use 1ms as
        // a threshold for the duration of the wait functions and fallback to
        // Sleep() if it returns before that. This could happen during normal
        // operation for the first call after the vsync thread becomes non-idle,
        // but it shouldn't happen often.
        if !wait_succeeded || elapsed < VSYNC_INTERVAL_THRESHOLD {
            log::trace!("VSyncProvider::wait_for_vsync() took less time than expected");
            // Re-query the interval on fallback path since this is where a stale
            // interval would cause incorrect timing.
            self.interval = query_dwm_interval();
            std::thread::sleep(self.interval);
        }
    }
}

/// Query the current DWM composition interval, falling back to the default 60Hz interval.
fn query_dwm_interval() -> Duration {
    get_dwm_interval()
        .context("Failed to get DWM interval")
        .log_err()
        .unwrap_or(DEFAULT_VSYNC_INTERVAL)
}

/// Get the current display refresh rate in Hz from DWM timing info.
/// Returns `None` if the DWM timing info cannot be retrieved.
pub(crate) fn get_display_refresh_rate_hz() -> Option<f32> {
    let mut timing_info = DWM_TIMING_INFO {
        cbSize: std::mem::size_of::<DWM_TIMING_INFO>() as u32,
        ..Default::default()
    };
    unsafe { DwmGetCompositionTimingInfo(HWND::default(), &mut timing_info) }.ok()?;

    let rate = if timing_info.rateRefresh.uiDenominator > 0 {
        timing_info.rateRefresh.uiNumerator as f32 / timing_info.rateRefresh.uiDenominator as f32
    } else {
        // Fall back to computing from qpcRefreshPeriod
        let period_us =
            timing_info.qpcRefreshPeriod as f64 / (*QPC_TICKS_PER_SECOND as f64 / 1_000_000.0);
        if period_us > 0.0 {
            (1_000_000.0 / period_us) as f32
        } else {
            return None;
        }
    };

    if rate > 0.0 && rate.is_finite() {
        Some(rate)
    } else {
        None
    }
}

fn get_dwm_interval() -> Result<Duration> {
    let mut timing_info = DWM_TIMING_INFO {
        cbSize: std::mem::size_of::<DWM_TIMING_INFO>() as u32,
        ..Default::default()
    };
    unsafe { DwmGetCompositionTimingInfo(HWND::default(), &mut timing_info) }?;
    let interval = retrieve_duration(timing_info.qpcRefreshPeriod, *QPC_TICKS_PER_SECOND);
    // Check for interval values that are impossibly low. A 29 microsecond
    // interval was seen (from a qpcRefreshPeriod of 60).
    if interval < VSYNC_INTERVAL_THRESHOLD {
        Ok(retrieve_duration(
            timing_info.rateRefresh.uiDenominator as u64,
            timing_info.rateRefresh.uiNumerator as u64,
        ))
    } else {
        Ok(interval)
    }
}

#[inline]
fn retrieve_duration(counts: u64, ticks_per_second: u64) -> Duration {
    let ticks_per_microsecond = ticks_per_second / 1_000_000;
    Duration::from_micros(counts / ticks_per_microsecond)
}
