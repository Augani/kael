use std::{
    fs,
    io::{BufRead as _, BufReader},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use super::dbus_util::parse_dbus_uint32;
use super::platform::PowerSaveHandle;
use crate::SystemPowerEvent;

pub(crate) fn power_mode() -> crate::PowerMode {
    if low_power_profile_enabled() {
        crate::PowerMode::LowPower
    } else {
        match on_battery_power() {
            Some(true) => crate::PowerMode::Balanced,
            Some(false) | None => crate::PowerMode::Performance,
        }
    }
}

fn low_power_profile_enabled() -> bool {
    read_trimmed("/sys/firmware/acpi/platform_profile")
        .map(|profile| {
            matches!(
                profile.as_str(),
                "low-power"
                    | "low_power"
                    | "power-saver"
                    | "powersaver"
                    | "battery-saver"
                    | "battery_saver"
            )
        })
        .unwrap_or(false)
}

fn on_battery_power() -> Option<bool> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    let mut saw_battery = false;

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(kind) = read_trimmed(path.join("type")) else {
            continue;
        };

        match kind.as_str() {
            "Battery" => saw_battery = true,
            "AC" | "Mains" | "USB" | "USB_C" | "USB_PD" | "USB_DCP" | "USB_CDP" | "USB_ACA" => {
                if read_trimmed(path.join("online")).as_deref() == Some("1") {
                    return Some(false);
                }
            }
            _ => {}
        }
    }

    saw_battery.then_some(true)
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
}

/// Inhibit the screensaver via `org.freedesktop.ScreenSaver.Inhibit` D-Bus call.
/// Returns a cookie that can be used to release the inhibition.
fn inhibit_screensaver_dbus(app_name: &str, reason: &str) -> Option<u32> {
    let output = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.ScreenSaver",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver.Inhibit",
            &format!("string:{}", app_name),
            &format!("string:{}", reason),
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_dbus_uint32(&output.stdout)
}

/// Release a screensaver inhibition via `org.freedesktop.ScreenSaver.UnInhibit`.
fn uninhibit_screensaver_dbus(cookie: u32) {
    let _ = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.ScreenSaver",
            "--type=method_call",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver.UnInhibit",
            &format!("uint32:{}", cookie),
        ])
        .output();
}

/// Inhibit via `org.freedesktop.login1.Manager.Inhibit` using the `systemd-inhibit`
/// CLI wrapper. The `what` parameter specifies what to inhibit (e.g. "idle", "sleep",
/// "idle:sleep"). The inhibit lock is held as long as the child process is alive.
fn inhibit_logind(what: &str, app_name: &str, reason: &str) -> Option<std::process::Child> {
    let what_arg = format!("--what={}", what);
    let who_arg = format!("--who={}", app_name);
    let why_arg = format!("--why={}", reason);
    #[allow(clippy::disallowed_methods)]
    let child = std::process::Command::new("systemd-inhibit")
        .args([&what_arg, &who_arg, &why_arg, "sleep", "infinity"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    Some(child)
}

/// Inhibit display sleep using both `org.freedesktop.ScreenSaver.Inhibit` and
/// `org.freedesktop.login1.Manager.Inhibit` (via systemd-inhibit) with idle inhibition.
/// This dual approach ensures coverage across different desktop environments.
pub fn inhibit_screensaver(app_name: &str, reason: &str) -> Option<PowerSaveHandle> {
    let cookie = inhibit_screensaver_dbus(app_name, reason);
    let logind_child = inhibit_logind("idle", app_name, reason);

    // Succeed if at least one inhibit method worked
    if cookie.is_none() && logind_child.is_none() {
        return None;
    }

    Some(PowerSaveHandle::DisplaySleep {
        screensaver_cookie: cookie,
        logind_child,
    })
}

/// Inhibit system suspend via `org.freedesktop.login1.Manager.Inhibit`
/// (using systemd-inhibit CLI wrapper) with sleep inhibition.
pub fn inhibit_suspend(app_name: &str, reason: &str) -> Option<PowerSaveHandle> {
    let child = inhibit_logind("sleep", app_name, reason)?;
    Some(PowerSaveHandle::SuspendChild(child))
}

/// Release a power save blocker, cleaning up all associated resources.
pub fn release_blocker(handle: PowerSaveHandle) {
    match handle {
        PowerSaveHandle::DisplaySleep {
            screensaver_cookie,
            logind_child,
        } => {
            if let Some(cookie) = screensaver_cookie {
                uninhibit_screensaver_dbus(cookie);
            }
            if let Some(mut child) = logind_child {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        PowerSaveHandle::SuspendChild(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Query system idle time via `org.freedesktop.ScreenSaver.GetSessionIdleTime` D-Bus call.
/// This works on both X11 and Wayland when a screensaver D-Bus service is available.
pub fn system_idle_time_dbus() -> Option<Duration> {
    let output = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.ScreenSaver",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver.GetSessionIdleTime",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_dbus_uint32(&output.stdout).map(|ms| Duration::from_millis(ms as u64))
}

/// Start background D-Bus monitors for Linux power and lock-screen events.
///
/// Returns a handle that keeps the monitors alive. Dropping the handle stops
/// the monitor threads and terminates the child `dbus-monitor` processes.
pub(crate) fn start_system_power_monitor(
    tx: calloop::channel::Sender<SystemPowerEvent>,
) -> Option<SystemPowerMonitorHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let mut pids = Vec::new();

    if let Some(pid) = spawn_logind_monitor(tx.clone(), stop.clone()) {
        pids.push(pid);
    }

    if let Some(pid) = spawn_screensaver_monitor(tx.clone(), stop.clone()) {
        pids.push(pid);
    }

    if let Some(pid) = spawn_upower_monitor(tx, stop.clone()) {
        pids.push(pid);
    }

    if pids.is_empty() {
        None
    } else {
        Some(SystemPowerMonitorHandle { pids, stop })
    }
}

/// Handle that keeps Linux system-power monitors alive.
pub(crate) struct SystemPowerMonitorHandle {
    pids: Vec<u32>,
    stop: Arc<AtomicBool>,
}

impl Drop for SystemPowerMonitorHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for pid in &self.pids {
            unsafe {
                libc::kill(*pid as i32, libc::SIGTERM);
            }
        }
    }
}

fn spawn_logind_monitor(
    tx: calloop::channel::Sender<SystemPowerEvent>,
    stop: Arc<AtomicBool>,
) -> Option<u32> {
    #[allow(clippy::disallowed_methods)]
    let child = std::process::Command::new("dbus-monitor")
        .args([
            "--system",
            "type='signal',sender='org.freedesktop.login1',interface='org.freedesktop.login1.Manager',member='PrepareForSleep'",
            "type='signal',sender='org.freedesktop.login1',interface='org.freedesktop.login1.Manager',member='PrepareForShutdown'",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let pid = child.id();
    let stdout = child.stdout?;

    std::thread::Builder::new()
        .name("power-logind-monitor".into())
        .spawn(move || {
            let reader = BufReader::new(stdout);
            let mut pending_signal = None::<LogindSignal>;

            for line in reader.lines() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                let Ok(line) = line else { break };
                let trimmed = line.trim();

                if trimmed.contains("member=PrepareForSleep") {
                    pending_signal = Some(LogindSignal::PrepareForSleep);
                    continue;
                }

                if trimmed.contains("member=PrepareForShutdown") {
                    pending_signal = Some(LogindSignal::PrepareForShutdown);
                    continue;
                }

                if let Some(boolean_value) = trimmed.strip_prefix("boolean")
                    && let Some(signal) = pending_signal.take()
                {
                    let event = match (signal, boolean_value.trim()) {
                        (LogindSignal::PrepareForSleep, "true") => Some(SystemPowerEvent::Suspend),
                        (LogindSignal::PrepareForSleep, "false") => Some(SystemPowerEvent::Resume),
                        (LogindSignal::PrepareForShutdown, "true") => {
                            Some(SystemPowerEvent::Shutdown)
                        }
                        _ => None,
                    };

                    if let Some(event) = event {
                        let _ = tx.send(event);
                    }
                }
            }
        })
        .ok()?;

    Some(pid)
}

fn spawn_screensaver_monitor(
    tx: calloop::channel::Sender<SystemPowerEvent>,
    stop: Arc<AtomicBool>,
) -> Option<u32> {
    #[allow(clippy::disallowed_methods)]
    let child = std::process::Command::new("dbus-monitor")
        .args([
            "--session",
            "type='signal',interface='org.freedesktop.ScreenSaver',member='ActiveChanged'",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let pid = child.id();
    let stdout = child.stdout?;

    std::thread::Builder::new()
        .name("power-screensaver-monitor".into())
        .spawn(move || {
            let reader = BufReader::new(stdout);
            let mut awaiting_active_state = false;

            for line in reader.lines() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                let Ok(line) = line else { break };
                let trimmed = line.trim();

                if trimmed.contains("member=ActiveChanged") {
                    awaiting_active_state = true;
                    continue;
                }

                if awaiting_active_state
                    && let Some(boolean_value) = trimmed.strip_prefix("boolean")
                {
                    awaiting_active_state = false;
                    let event = match boolean_value.trim() {
                        "true" => Some(SystemPowerEvent::LockScreen),
                        "false" => Some(SystemPowerEvent::UnlockScreen),
                        _ => None,
                    };
                    if let Some(event) = event {
                        let _ = tx.send(event);
                    }
                }
            }
        })
        .ok()?;

    Some(pid)
}

fn spawn_upower_monitor(
    tx: calloop::channel::Sender<SystemPowerEvent>,
    stop: Arc<AtomicBool>,
) -> Option<u32> {
    #[allow(clippy::disallowed_methods)]
    let child = std::process::Command::new("dbus-monitor")
        .args([
            "--system",
            "type='signal',sender='org.freedesktop.UPower',path='/org/freedesktop/UPower/devices/DisplayDevice',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let pid = child.id();
    let stdout = child.stdout?;

    std::thread::Builder::new()
        .name("power-upower-monitor".into())
        .spawn(move || {
            let reader = BufReader::new(stdout);

            for line in reader.lines() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                let Ok(line) = line else { break };
                if line.trim().contains("member=PropertiesChanged") {
                    let _ = tx.send(SystemPowerEvent::PowerModeChanged);
                }
            }
        })
        .ok()?;

    Some(pid)
}

#[derive(Clone, Copy)]
enum LogindSignal {
    PrepareForSleep,
    PrepareForShutdown,
}
