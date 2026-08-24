//! Pure state machines for the browser AudioWorklet bridge.
//!
//! Keeping browser-provided counters and queue requests out of the Web API
//! adapter makes the pressure protocol injectable in ordinary native tests.

const MAX_SAFE_JS_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputRequest {
    pub(crate) chunks: usize,
    pub(crate) rendered_frames: Option<u64>,
    pub(crate) underrun_frames: Option<u64>,
}

pub(crate) fn bounded_output_request(
    chunks: Option<f64>,
    rendered_frames: Option<f64>,
    underrun_frames: Option<f64>,
    queue_limit: usize,
) -> OutputRequest {
    OutputRequest {
        chunks: bounded_count(chunks, queue_limit),
        rendered_frames: bounded_counter(rendered_frames),
        underrun_frames: bounded_counter(underrun_frames),
    }
}

fn bounded_count(value: Option<f64>, limit: usize) -> usize {
    let limit = limit.max(1);
    value
        .filter(|value| value.is_finite())
        .unwrap_or(1.0)
        .clamp(1.0, limit as f64) as usize
}

fn bounded_counter(value: Option<f64>) -> Option<u64> {
    value
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, MAX_SAFE_JS_INTEGER) as u64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureDeliveryError {
    InvalidSampleCount,
    MissingSequence,
    OutOfOrder,
    MissingDropCounter,
    RegressedDropCounter,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureDeliveryState {
    expected_sequence: u64,
    dropped_frames: u64,
}

impl CaptureDeliveryState {
    pub(crate) fn accept(
        &mut self,
        actual_samples: usize,
        expected_samples: usize,
        sequence: Option<f64>,
        dropped_frames: Option<f64>,
    ) -> Result<Option<u64>, CaptureDeliveryError> {
        if actual_samples != expected_samples {
            return Err(CaptureDeliveryError::InvalidSampleCount);
        }
        let sequence = bounded_counter(sequence).ok_or(CaptureDeliveryError::MissingSequence)?;
        if sequence != self.expected_sequence {
            return Err(CaptureDeliveryError::OutOfOrder);
        }
        let dropped_frames =
            bounded_counter(dropped_frames).ok_or(CaptureDeliveryError::MissingDropCounter)?;
        if dropped_frames < self.dropped_frames {
            return Err(CaptureDeliveryError::RegressedDropCounter);
        }
        self.expected_sequence = self.expected_sequence.saturating_add(1);
        let changed = (dropped_frames > self.dropped_frames).then_some(dropped_frames);
        self.dropped_frames = dropped_frames;
        Ok(changed)
    }

    pub(crate) fn dropped_frames(self) -> u64 {
        self.dropped_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_requests_are_injected_and_clamped_to_queue_capacity() {
        assert_eq!(
            bounded_output_request(Some(1_000.0), Some(256.0), Some(2.0), 4),
            OutputRequest {
                chunks: 4,
                rendered_frames: Some(256),
                underrun_frames: Some(2),
            }
        );
        assert_eq!(
            bounded_output_request(Some(f64::NAN), Some(-1.0), None, 4),
            OutputRequest {
                chunks: 1,
                rendered_frames: Some(0),
                underrun_frames: None,
            }
        );
    }

    #[test]
    fn capture_delivery_requires_exact_order_size_and_monotonic_pressure() {
        let mut state = CaptureDeliveryState::default();
        assert_eq!(state.accept(256, 256, Some(0.0), Some(0.0)), Ok(None));
        assert_eq!(
            state.accept(256, 256, Some(1.0), Some(128.0)),
            Ok(Some(128))
        );
        assert_eq!(state.dropped_frames(), 128);
        assert_eq!(
            state.accept(255, 256, Some(2.0), Some(128.0)),
            Err(CaptureDeliveryError::InvalidSampleCount)
        );
        assert_eq!(
            state.accept(256, 256, Some(3.0), Some(128.0)),
            Err(CaptureDeliveryError::OutOfOrder)
        );
        assert_eq!(
            state.accept(256, 256, Some(2.0), Some(64.0)),
            Err(CaptureDeliveryError::RegressedDropCounter)
        );
    }

    #[test]
    fn capture_delivery_rejects_missing_browser_counters() {
        let mut state = CaptureDeliveryState::default();
        assert_eq!(
            state.accept(128, 128, None, Some(0.0)),
            Err(CaptureDeliveryError::MissingSequence)
        );
        assert_eq!(
            state.accept(128, 128, Some(0.0), None),
            Err(CaptureDeliveryError::MissingDropCounter)
        );
    }
}
