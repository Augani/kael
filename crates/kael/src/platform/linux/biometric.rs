use crate::{BiometricKind, BiometricStatus};

/// Check if fprintd is available and has enrolled fingerprints.
///
/// Queries the `net.reactivated.Fprint.Manager` D-Bus interface to list devices,
/// then checks if any device has enrolled fingerprints for the current user.
fn fprintd_status() -> Option<BiometricStatus> {
    // Check if fprintd manager is reachable by listing devices
    let output = std::process::Command::new("dbus-send")
        .args([
            "--system",
            "--dest=net.reactivated.Fprint",
            "--type=method_call",
            "--print-reply",
            "/net/reactivated/Fprint/Manager",
            "net.reactivated.Fprint.Manager.GetDefaultDevice",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = std::str::from_utf8(&output.stdout).ok()?;
    let device_path = parse_dbus_object_path(stdout)?;

    // Check if the current user has enrolled fingerprints on this device
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_default();

    if username.is_empty() {
        return Some(BiometricStatus::Available(BiometricKind::Fingerprint));
    }

    let list_output = std::process::Command::new("dbus-send")
        .args([
            "--system",
            "--dest=net.reactivated.Fprint",
            "--type=method_call",
            "--print-reply",
            &device_path,
            "net.reactivated.Fprint.Device.ListEnrolledFingers",
            &format!("string:{}", username),
        ])
        .output()
        .ok()?;

    if list_output.status.success() {
        let list_stdout = std::str::from_utf8(&list_output.stdout).ok()?;
        // If the response contains array elements, fingerprints are enrolled
        if list_stdout.contains("string") {
            Some(BiometricStatus::Available(BiometricKind::Fingerprint))
        } else {
            // Device exists but no fingerprints enrolled — still report available
            // since the hardware is present
            Some(BiometricStatus::Available(BiometricKind::Fingerprint))
        }
    } else {
        // Device exists but listing failed (e.g., no enrolled fingers error)
        // The hardware is present, so report available
        Some(BiometricStatus::Available(BiometricKind::Fingerprint))
    }
}

/// Check if polkit is available for general authentication.
fn polkit_available() -> bool {
    let output = std::process::Command::new("dbus-send")
        .args([
            "--system",
            "--dest=org.freedesktop.PolicyKit1",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/PolicyKit1/Authority",
            "org.freedesktop.DBus.Properties.Get",
            "string:org.freedesktop.PolicyKit1.Authority",
            "string:BackendVersion",
        ])
        .output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Returns the biometric status for the Linux platform.
///
/// Checks fprintd first for fingerprint reader support, then falls back
/// to polkit for general authentication capability.
pub fn biometric_status() -> BiometricStatus {
    // Try fprintd first
    if let Some(status) = fprintd_status() {
        return status;
    }

    // Polkit doesn't provide biometric auth per se, but it can prompt
    // for authentication. We only report it as available if fprintd is not.
    // Since polkit is password-based, not biometric, we report Unavailable
    // when there's no actual biometric hardware.
    BiometricStatus::Unavailable
}

/// Authenticate using fprintd fingerprint verification.
fn authenticate_fprintd(reason: &str, callback: Box<dyn FnOnce(bool) + Send>) {
    let _ = reason; // fprintd doesn't use a reason string in its D-Bus API

    // Use fprintd-verify which handles the full verification flow
    let verified = std::process::Command::new("fprintd-verify")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    complete_authentication(callback, verified);
}

/// Authenticate using polkit (pkexec or pkttyagent).
///
/// Uses `pkcheck` to verify the user's identity via polkit, which will
/// prompt for password authentication through the system's polkit agent.
fn authenticate_polkit(reason: &str, callback: Box<dyn FnOnce(bool) + Send>) {
    let _ = reason;

    // Use pkcheck with a well-known action that requires auth
    let pid = std::process::id();
    let verified = std::process::Command::new("pkcheck")
        .args([
            "--action-id",
            "org.freedesktop.policykit.exec",
            "--process",
            &pid.to_string(),
            "--allow-user-interaction",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    complete_authentication(callback, verified);
}

/// Authenticate the user via biometrics on Linux.
///
/// Tries fprintd first if available, then falls back to polkit.
pub fn authenticate_biometric(reason: &str, callback: Box<dyn FnOnce(bool) + Send>) {
    // If fprintd is available, use fingerprint verification
    if fprintd_status().is_some() {
        authenticate_fprintd(reason, callback);
        return;
    }

    // Fall back to polkit for general authentication
    if polkit_available() {
        authenticate_polkit(reason, callback);
        return;
    }

    // No authentication method available
    complete_authentication(callback, false);
}

fn complete_authentication(callback: Box<dyn FnOnce(bool) + Send>, verified: bool) {
    crate::platform::catch_platform_callback("Linux", "biometric authentication", (), || {
        callback(verified)
    });
}

/// Parse a D-Bus object path from dbus-send --print-reply output.
fn parse_dbus_object_path(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("object path") {
            let path = rest.trim().trim_matches('"');
            if path.starts_with('/') {
                return Some(path.to_string());
            }
        }
    }
    None
}
