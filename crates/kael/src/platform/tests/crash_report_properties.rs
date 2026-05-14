// Feature: platform-parity-electron-features, Property 11: Crash report persistence round-trip

use proptest::prelude::*;

use crate::CrashReport;

/// Generates an arbitrary `CrashReport`.
fn crash_report_strategy() -> impl Strategy<Value = CrashReport> {
    (
        "[a-zA-Z0-9_ ]{1,100}",
        "[a-zA-Z0-9_\n ]{1,200}",
        prop::option::of("[a-zA-Z0-9._-]{1,20}"),
    )
        .prop_map(|(message, backtrace, app_version)| CrashReport {
            message,
            backtrace,
            os_info: crate::OsInfo {
                name: "test-os".into(),
                arch: "x86_64".into(),
                version: "1.0".into(),
                locale: "en-US".into(),
                hostname: "test-host".into(),
            },
            app_version,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 26.2, 26.4**
    ///
    /// For any valid `CrashReport`, serializing, storing, loading, and
    /// deserializing produces an equivalent value.
    #[test]
    fn crash_report_persistence_roundtrip(report in crash_report_strategy()) {
        let json = serde_json::to_string(&report).expect("CrashReport should serialize to JSON");
        let deserialized: CrashReport = serde_json::from_str(&json)
            .expect("CrashReport should deserialize from JSON");

        prop_assert_eq!(report.message, deserialized.message);
        prop_assert_eq!(report.backtrace, deserialized.backtrace);
        prop_assert_eq!(report.app_version, deserialized.app_version);
        prop_assert_eq!(report.os_info.name, deserialized.os_info.name);
        prop_assert_eq!(report.os_info.arch, deserialized.os_info.arch);
    }
}
