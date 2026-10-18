/// An actual GPU submission measured by an explicitly enabled renderer.
///
/// The Metal backend reports seconds on the system's monotonic host clock, the
/// same clock used by `CACurrentMediaTime`. These values are neither wall-clock
/// timestamps nor comparable with Rust `Instant` values. GPU timestamps are
/// available only after command-buffer completion. Presentation is available
/// only after the drawable's presented callback; offscreen submissions have no
/// presentation timestamp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuFrameTiming {
    /// A monotonically increasing identifier within one telemetry session.
    pub frame_id: u64,
    /// The host-clock time immediately before command-buffer submission.
    pub submitted_time_seconds: f64,
    /// When the GPU began executing this command buffer.
    pub gpu_start_time_seconds: f64,
    /// When the GPU finished executing this command buffer.
    pub gpu_end_time_seconds: f64,
    /// When the drawable was displayed, or `None` for an offscreen or dropped frame.
    pub presented_time_seconds: Option<f64>,
}

#[cfg(any(test, all(target_os = "macos", not(feature = "macos-blade"))))]
pub(crate) mod collector {
    use super::GpuFrameTiming;
    use std::collections::VecDeque;

    pub(crate) const MAX_GPU_FRAME_TIMINGS: usize = 64;

    struct PendingFrame {
        timing: GpuFrameTiming,
        completed: bool,
        awaiting_presentation: bool,
    }

    /// This collector owns no GPU resources and never wakes the application.
    /// Its callbacks use weak references so disabling telemetry retires a session.
    pub(crate) struct GpuFrameTimingCollector {
        next_frame_id: u64,
        records: VecDeque<PendingFrame>,
    }

    impl GpuFrameTimingCollector {
        pub(crate) fn new() -> Self {
            Self {
                next_frame_id: 0,
                records: VecDeque::with_capacity(MAX_GPU_FRAME_TIMINGS),
            }
        }

        pub(crate) fn begin(&mut self, submitted: f64, has_drawable: bool) -> Option<u64> {
            let frame_id = self.next_frame_id;
            self.next_frame_id = frame_id.checked_add(1)?;
            if self.records.len() == MAX_GPU_FRAME_TIMINGS {
                self.records.pop_front();
            }
            self.records.push_back(PendingFrame {
                timing: GpuFrameTiming {
                    frame_id,
                    submitted_time_seconds: submitted,
                    gpu_start_time_seconds: 0.0,
                    gpu_end_time_seconds: 0.0,
                    presented_time_seconds: None,
                },
                completed: false,
                awaiting_presentation: has_drawable,
            });
            Some(frame_id)
        }

        pub(crate) fn complete(&mut self, frame_id: u64, gpu_start: f64, gpu_end: f64) {
            if !gpu_start.is_finite()
                || gpu_start <= 0.0
                || !gpu_end.is_finite()
                || gpu_end < gpu_start
            {
                // Failed command buffers do not have usable GPU timestamps.
                self.records
                    .retain(|record| record.timing.frame_id != frame_id);
                return;
            }
            if let Some(record) = self
                .records
                .iter_mut()
                .find(|r| r.timing.frame_id == frame_id)
            {
                record.timing.gpu_start_time_seconds = gpu_start;
                record.timing.gpu_end_time_seconds = gpu_end;
                record.completed = true;
            }
        }

        pub(crate) fn submitted(&mut self, frame_id: u64, time: f64) {
            if let Some(record) = self
                .records
                .iter_mut()
                .find(|r| r.timing.frame_id == frame_id)
            {
                record.timing.submitted_time_seconds = time;
            }
        }

        pub(crate) fn presented(&mut self, frame_id: u64, presented: f64) {
            if let Some(record) = self
                .records
                .iter_mut()
                .find(|r| r.timing.frame_id == frame_id)
            {
                record.timing.presented_time_seconds =
                    (presented.is_finite() && presented > 0.0).then_some(presented);
                record.awaiting_presentation = false;
            }
        }

        pub(crate) fn take(&mut self) -> Vec<GpuFrameTiming> {
            let mut result = Vec::new();
            self.records.retain(|record| {
                if record.completed && !record.awaiting_presentation {
                    result.push(record.timing);
                    false
                } else {
                    true
                }
            });
            result
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn presentation_and_completion_can_arrive_in_either_order_without_losing_times() {
            let mut collector = GpuFrameTimingCollector::new();
            let first = collector.begin(10.0, true).unwrap();
            let second = collector.begin(11.0, true).unwrap();
            collector.submitted(first, 10.01);
            collector.complete(first, 10.1, 10.2);
            collector.presented(second, 11.3);
            assert!(collector.take().is_empty());
            collector.complete(second, 11.1, 11.2);
            collector.presented(first, 10.3);
            let records = collector.take();
            assert_eq!(records.len(), 2);
            assert_eq!(records[0].submitted_time_seconds, 10.01);
            assert_eq!(records[0].presented_time_seconds, Some(10.3));
            assert_eq!(records[1].presented_time_seconds, Some(11.3));
            assert!(collector.take().is_empty());
        }

        #[test]
        fn pending_and_completed_frames_share_one_bounded_capacity() {
            let mut collector = GpuFrameTimingCollector::new();
            let first = collector.begin(1.0, true).unwrap();
            for frame in 1..=MAX_GPU_FRAME_TIMINGS {
                let id = collector.begin(frame as f64 + 1.0, false).unwrap();
                collector.complete(id, frame as f64 + 1.1, frame as f64 + 1.2);
            }
            collector.complete(first, 1.1, 1.2);
            collector.presented(first, 1.3);
            assert_eq!(collector.records.len(), MAX_GPU_FRAME_TIMINGS);
            let records = collector.take();
            assert_eq!(records.len(), MAX_GPU_FRAME_TIMINGS);
            assert_eq!(records[0].frame_id, 1);
        }

        #[test]
        fn failed_gpu_commands_are_discarded_and_dropped_presentations_remain_explicit() {
            let mut collector = GpuFrameTimingCollector::new();
            let failed = collector.begin(1.0, false).unwrap();
            collector.complete(failed, 0.0, 0.0);
            let dropped = collector.begin(2.0, true).unwrap();
            collector.complete(dropped, 2.1, 2.2);
            collector.presented(dropped, 0.0);
            let records = collector.take();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].frame_id, dropped);
            assert_eq!(records[0].presented_time_seconds, None);
        }
    }
}
