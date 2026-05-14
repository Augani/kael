use std::process::Command;

/// Attempt to open the system character/emoji picker on Linux.
///
/// Detection order:
/// 1. IBus emoji panel via D-Bus
/// 2. Fcitx5 emoji addon via D-Bus
/// 3. Fallback to `gnome-characters` application
pub fn show_character_palette() {
    if try_ibus_emoji() {
        return;
    }
    if try_fcitx5_emoji() {
        return;
    }
    try_gnome_characters();
}

/// Try to invoke the IBus emoji picker via D-Bus.
fn try_ibus_emoji() -> bool {
    // Check if IBus daemon is running by querying the bus.
    let status = Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.IBus",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/IBus",
            "org.freedesktop.IBus.Panel.Extension.Emoji",
        ])
        .output();
    matches!(status, Ok(output) if output.status.success())
}

/// Try to invoke the Fcitx5 emoji picker via D-Bus.
fn try_fcitx5_emoji() -> bool {
    let status = Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.fcitx.Fcitx5",
            "--type=method_call",
            "--print-reply",
            "/controller",
            "org.fcitx.Fcitx.Controller1.Activate",
            "string:emoji",
        ])
        .output();
    matches!(status, Ok(output) if output.status.success())
}

/// Fallback: launch `gnome-characters` as a standalone picker.
fn try_gnome_characters() {
    #[allow(clippy::disallowed_methods)]
    let _ = Command::new("gnome-characters").spawn();
}
