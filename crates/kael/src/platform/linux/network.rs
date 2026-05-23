use std::sync::Arc;

use super::dbus_util::parse_dbus_uint32;
use crate::NetworkStatus;

/// NetworkManager D-Bus states.
/// See: https://developer-old.gnome.org/NetworkManager/stable/nm-dbus-types.html
/// NM_STATE_CONNECTED_GLOBAL = 70 means full network connectivity.
/// States >= 70 are considered online.
const NM_STATE_CONNECTED_GLOBAL: u32 = 70;

/// Query the current network status via NetworkManager D-Bus interface.
/// Falls back to sysfs if NetworkManager is not available.
pub(crate) fn query_network_status() -> NetworkStatus {
    if let Some(status) = query_network_manager_state() {
        return status;
    }
    // Fallback to sysfs when NetworkManager is not available
    network_status_from_sysfs()
}

/// Query NetworkManager's current state via `org.freedesktop.NetworkManager.state()`.
fn query_network_manager_state() -> Option<NetworkStatus> {
    let output = std::process::Command::new("dbus-send")
        .args([
            "--system",
            "--dest=org.freedesktop.NetworkManager",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/NetworkManager",
            "org.freedesktop.NetworkManager.state",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let state = parse_dbus_uint32(&output.stdout)?;
    Some(nm_state_to_network_status(state))
}

/// Convert a NetworkManager state integer to a `NetworkStatus`.
fn nm_state_to_network_status(state: u32) -> NetworkStatus {
    if state >= NM_STATE_CONNECTED_GLOBAL {
        NetworkStatus::Online
    } else {
        NetworkStatus::Offline
    }
}

/// Start a background thread that monitors NetworkManager's `StateChanged` D-Bus signal.
///
/// When the signal fires, the new `NetworkStatus` is sent through the provided
/// `calloop::channel::Sender` so the main event loop can invoke the callback
/// on the correct thread.
///
/// Returns a handle that keeps the monitor alive. Drop it to stop monitoring.
/// Returns `None` if `dbus-monitor` cannot be spawned (e.g. NetworkManager absent).
pub(crate) fn start_network_monitor(
    tx: calloop::channel::Sender<NetworkStatus>,
) -> Option<NetworkMonitorHandle> {
    // Spawn dbus-monitor to listen for the StateChanged signal on the system bus.
    #[allow(clippy::disallowed_methods)]
    let child = std::process::Command::new("dbus-monitor")
        .args([
            "--system",
            "type='signal',interface='org.freedesktop.NetworkManager',member='StateChanged'",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let pid = child.id();
    let stdout = child.stdout;

    // Spawn a reader thread that parses dbus-monitor output for state changes.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_clone = stop.clone();

    std::thread::Builder::new()
        .name("network-monitor".into())
        .spawn(move || {
            use std::io::{BufRead, BufReader};

            let Some(stdout) = stdout else { return };
            let reader = BufReader::new(stdout);

            for line in reader.lines() {
                if stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let Ok(line) = line else { break };
                // dbus-monitor prints signal arguments like:
                //    uint32 70
                // We look for uint32 lines following a StateChanged signal header.
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("uint32") {
                    if let Ok(state) = rest.trim().parse::<u32>() {
                        let status = nm_state_to_network_status(state);
                        // Send to the main event loop; ignore errors (channel closed).
                        let _ = tx.send(status);
                    }
                }
            }
        })
        .ok()?;

    Some(NetworkMonitorHandle { pid, stop })
}

/// Handle that keeps the network monitor background process alive.
/// Dropping it kills the `dbus-monitor` child process and signals the reader thread to stop.
pub(crate) struct NetworkMonitorHandle {
    pid: u32,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for NetworkMonitorHandle {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        // Kill the dbus-monitor child process.
        unsafe {
            libc::kill(self.pid as i32, libc::SIGTERM);
        }
    }
}

/// Fallback: read network status from sysfs when NetworkManager is not available.
fn network_status_from_sysfs() -> NetworkStatus {
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name == "lo" {
                continue;
            }
            if let Ok(state) = std::fs::read_to_string(entry.path().join("operstate")) {
                if state.trim() == "up" {
                    return NetworkStatus::Online;
                }
            }
        }
    }
    NetworkStatus::Offline
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nm_state_to_network_status() {
        // NM_STATE_UNKNOWN = 0
        assert_eq!(nm_state_to_network_status(0), NetworkStatus::Offline);
        // NM_STATE_ASLEEP = 10
        assert_eq!(nm_state_to_network_status(10), NetworkStatus::Offline);
        // NM_STATE_DISCONNECTED = 20
        assert_eq!(nm_state_to_network_status(20), NetworkStatus::Offline);
        // NM_STATE_DISCONNECTING = 30
        assert_eq!(nm_state_to_network_status(30), NetworkStatus::Offline);
        // NM_STATE_CONNECTING = 40
        assert_eq!(nm_state_to_network_status(40), NetworkStatus::Offline);
        // NM_STATE_CONNECTED_LOCAL = 50
        assert_eq!(nm_state_to_network_status(50), NetworkStatus::Offline);
        // NM_STATE_CONNECTED_SITE = 60
        assert_eq!(nm_state_to_network_status(60), NetworkStatus::Offline);
        // NM_STATE_CONNECTED_GLOBAL = 70
        assert_eq!(nm_state_to_network_status(70), NetworkStatus::Online);
        // Values above 70
        assert_eq!(nm_state_to_network_status(80), NetworkStatus::Online);
    }

    #[test]
    fn test_parse_dbus_uint32() {
        let output = b"method return time=1234.567 sender=:1.2 -> destination=:1.3\n   uint32 70\n";
        assert_eq!(parse_dbus_uint32(output), Some(70));

        let output = b"method return time=1234.567\n   uint32 20\n";
        assert_eq!(parse_dbus_uint32(output), Some(20));

        let output = b"no uint32 here\n";
        assert_eq!(parse_dbus_uint32(output), None);

        let output = b"";
        assert_eq!(parse_dbus_uint32(output), None);
    }

    #[test]
    fn test_sysfs_fallback_does_not_panic() {
        // This just verifies the sysfs fallback doesn't panic on any system.
        // On non-Linux systems it will return Offline (no /sys/class/net).
        let _status = network_status_from_sysfs();
    }
}
