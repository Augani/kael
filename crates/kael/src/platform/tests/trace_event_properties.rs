// Feature: platform-parity-electron-features, Property 12: Trace event Chrome format export

use proptest::prelude::*;

use crate::tracer::{TraceEvent, TracePhase};

/// Generates an arbitrary `TracePhase`.
fn trace_phase_strategy() -> impl Strategy<Value = TracePhase> {
    prop_oneof![
        Just(TracePhase::Begin),
        Just(TracePhase::End),
        Just(TracePhase::Instant),
        Just(TracePhase::Counter),
        Just(TracePhase::AsyncBegin),
        Just(TracePhase::AsyncEnd),
    ]
}

/// Generates an arbitrary `TraceEvent`.
fn trace_event_strategy() -> impl Strategy<Value = TraceEvent> {
    (
        "[a-zA-Z0-9_ ]{1,50}",
        "[a-zA-Z0-9_ ]{1,30}",
        trace_phase_strategy(),
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        prop::option::of(any::<u64>()),
    )
        .prop_map(
            |(name, category, phase, timestamp_us, process_id, thread_id, duration_us)| {
                TraceEvent {
                    name,
                    category,
                    phase,
                    timestamp_us,
                    process_id,
                    thread_id,
                    duration_us,
                    args: None,
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 27.2**
    ///
    /// For any valid `TraceEvent`, exporting to Chrome Trace Event JSON
    /// produces an object with required fields: `name`, `cat`, `ph`, `ts`,
    /// `pid`, `tid`.
    #[test]
    fn trace_event_chrome_format_export_has_required_fields(event in trace_event_strategy()) {
        let json = event.to_chrome_json().expect("TraceEvent should export to JSON");
        let value: serde_json::Value = serde_json::from_str(&json)
            .expect("exported trace event should be valid JSON");

        let obj = value.as_object().expect("exported trace event should be a JSON object");

        prop_assert!(obj.contains_key("name"), "missing required field: name");
        prop_assert!(obj.contains_key("cat"), "missing required field: cat");
        prop_assert!(obj.contains_key("ph"), "missing required field: ph");
        prop_assert!(obj.contains_key("ts"), "missing required field: ts");
        prop_assert!(obj.contains_key("pid"), "missing required field: pid");
        prop_assert!(obj.contains_key("tid"), "missing required field: tid");

        // Verify field types
        prop_assert!(obj["name"].is_string(), "name should be a string");
        prop_assert!(obj["cat"].is_string(), "cat should be a string");
        prop_assert!(obj["ph"].is_string(), "ph should be a string");
        prop_assert!(obj["ts"].is_number(), "ts should be a number");
        prop_assert!(obj["pid"].is_number(), "pid should be a number");
        prop_assert!(obj["tid"].is_number(), "tid should be a number");
    }
}
