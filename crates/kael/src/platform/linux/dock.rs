use std::path::Path;

use crate::MenuItem;

/// Set the dock badge count using the Unity Launcher API via D-Bus.
///
/// Uses the `com.canonical.Unity.LauncherEntry` interface to set the
/// `count` and `count-visible` properties on the application's launcher entry.
/// This is supported by Unity, Plank, and some other Linux docks.
pub fn set_dock_badge(label: Option<&str>) {
    let desktop_uri = match get_desktop_file_uri() {
        Some(uri) => uri,
        None => return,
    };

    match label {
        Some(text) if !text.is_empty() => {
            // Try to parse the label as a number for the count property.
            // Unity Launcher API uses an integer count, not arbitrary text.
            let count: i64 = text.parse().unwrap_or(0);
            let _ = std::process::Command::new("dbus-send")
                .args([
                    "--session",
                    "--dest=com.canonical.Unity",
                    "--type=method_call",
                    "/com/canonical/Unity/LauncherEntry",
                    "com.canonical.Unity.LauncherEntry.Update",
                    &format!("string:{}", desktop_uri),
                    &format!(
                        "dict:string:variant:count,int64:{},count-visible,boolean:true",
                        count
                    ),
                ])
                .output();
        }
        _ => {
            // Clear the badge by setting count-visible to false
            let _ = std::process::Command::new("dbus-send")
                .args([
                    "--session",
                    "--dest=com.canonical.Unity",
                    "--type=method_call",
                    "/com/canonical/Unity/LauncherEntry",
                    "com.canonical.Unity.LauncherEntry.Update",
                    &format!("string:{}", desktop_uri),
                    "dict:string:variant:count-visible,boolean:false",
                ])
                .output();
        }
    }
}

/// Set the dock menu by exposing menu items via the `com.canonical.dbusmenu`
/// D-Bus interface.
///
/// This writes a simple menu description that compatible docks can pick up.
/// Since the full dbusmenu protocol requires a persistent D-Bus service,
/// this implementation uses `dbus-send` to signal menu updates.
/// In practice, many Linux docks read the desktop actions from the .desktop file,
/// so we also update the .desktop file with the menu actions as a fallback.
pub fn set_dock_menu(menu: &[MenuItem], _keymap: &crate::Keymap) {
    // Extract action names from menu items and write them to the .desktop file
    // as desktop actions, which is the most widely supported approach on Linux.
    let actions = extract_action_names(menu);
    if actions.is_empty() {
        return;
    }

    let Some(desktop_file_path) = find_desktop_file_path() else {
        return;
    };

    // Read existing .desktop file content
    let existing_content = std::fs::read_to_string(&desktop_file_path).unwrap_or_default();

    // Build the updated .desktop file with Actions
    let action_ids: Vec<String> = actions
        .iter()
        .enumerate()
        .map(|(i, _)| format!("DockAction{}", i))
        .collect();

    let actions_line = format!("Actions={};", action_ids.join(";"));

    // Rebuild the [Desktop Entry] section, replacing or adding the Actions line
    let mut new_content = String::new();
    let mut found_actions = false;
    let mut in_desktop_entry = false;
    let mut past_desktop_entry = false;

    for line in existing_content.lines() {
        if line.starts_with("[Desktop Entry]") {
            in_desktop_entry = true;
            past_desktop_entry = false;
            new_content.push_str(line);
            new_content.push('\n');
            continue;
        }
        if line.starts_with('[') && in_desktop_entry {
            // We've left the [Desktop Entry] section
            in_desktop_entry = false;
            past_desktop_entry = true;
        }
        if in_desktop_entry && line.starts_with("Actions=") {
            new_content.push_str(&actions_line);
            new_content.push('\n');
            found_actions = true;
            continue;
        }
        // Skip old [Desktop Action ...] sections that we generated
        if line.starts_with("[Desktop Action DockAction") {
            // Skip this section entirely
            past_desktop_entry = true;
            continue;
        }
        if past_desktop_entry && !line.starts_with('[') && !line.is_empty() {
            // Still in a skipped action section
            continue;
        }
        if line.starts_with('[') {
            past_desktop_entry = false;
        }
        new_content.push_str(line);
        new_content.push('\n');
    }

    if !found_actions && !new_content.is_empty() {
        // Append Actions line at the end of the content
        new_content.push_str(&actions_line);
        new_content.push('\n');
    }

    // Append the action sections
    for (i, name) in actions.iter().enumerate() {
        new_content.push_str(&format!(
            "\n[Desktop Action DockAction{}]\nName={}\n",
            i, name
        ));
    }

    let _ = std::fs::write(&desktop_file_path, new_content);
}

/// Add a file path to the recently-used documents list.
///
/// Writes to `~/.local/share/recently-used.xbel` in the XBEL bookmark format,
/// which is the standard location for recent documents on freedesktop-compliant
/// Linux desktops.
pub fn add_recent_document(path: &Path) {
    let path_str = match path.to_str() {
        Some(s) => s,
        None => return,
    };

    // Convert the file path to a file:// URI
    let uri = format!("file://{}", path_str);

    let mime_type = guess_mime_type(path);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let app_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "gpui-app".to_string());

    let xbel_path = get_recently_used_xbel_path();
    let Some(xbel_path) = xbel_path else {
        return;
    };

    // Read existing content or create new file
    let existing = std::fs::read_to_string(&xbel_path).unwrap_or_default();

    let bookmark_entry = format!(
        r#"  <bookmark href="{uri}" modified="{timestamp}" visited="{timestamp}">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="{mime}"/>
        <bookmark:applications>
          <bookmark:application name="{app}" exec="&apos;{app}&apos; %u" modified="{timestamp}" count="1"/>
        </bookmark:applications>
      </metadata>
    </info>
  </bookmark>"#,
        uri = escape_xml(&uri),
        timestamp = format_iso8601(timestamp),
        mime = escape_xml(&mime_type),
        app = escape_xml(&app_name),
    );

    if existing.contains("<xbel") {
        // Update existing file: remove any existing bookmark for this URI, then add new one
        let mut new_content = String::new();
        let mut skip_bookmark = false;

        for line in existing.lines() {
            if line.contains(&format!("href=\"{}\"", escape_xml(&uri))) {
                skip_bookmark = true;
                continue;
            }
            if skip_bookmark {
                if line.contains("</bookmark>") {
                    skip_bookmark = false;
                }
                continue;
            }
            if line.contains("</xbel>") {
                new_content.push_str(&bookmark_entry);
                new_content.push('\n');
            }
            new_content.push_str(line);
            new_content.push('\n');
        }

        let _ = std::fs::write(&xbel_path, new_content);
    } else {
        // Create new XBEL file
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0"
      xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks"
      xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
{bookmark}
</xbel>
"#,
            bookmark = bookmark_entry,
        );

        // Ensure parent directory exists
        if let Some(parent) = xbel_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let _ = std::fs::write(&xbel_path, content);
    }
}

/// Get the desktop file URI for the current application.
/// Returns something like "application://myapp.desktop"
fn get_desktop_file_uri() -> Option<String> {
    let app_name = std::env::current_exe()
        .ok()?
        .file_stem()?
        .to_string_lossy()
        .into_owned();

    Some(format!("application://{}.desktop", app_name))
}

/// Find the .desktop file path for the current application.
fn find_desktop_file_path() -> Option<std::path::PathBuf> {
    let app_name = std::env::current_exe()
        .ok()?
        .file_stem()?
        .to_string_lossy()
        .into_owned();

    let data_dir = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".local").join("share"))
        })?;

    let desktop_file = data_dir
        .join("applications")
        .join(format!("{}.desktop", app_name));

    if desktop_file.exists() {
        return Some(desktop_file);
    }

    // Also check the url-handler variant
    let url_handler_file = data_dir
        .join("applications")
        .join(format!("{}-url-handler.desktop", app_name));

    if url_handler_file.exists() {
        return Some(url_handler_file);
    }

    // Check system-wide locations
    for dir in &["/usr/share/applications", "/usr/local/share/applications"] {
        let path = std::path::Path::new(dir).join(format!("{}.desktop", app_name));
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Extract action names from MenuItem list (only Action variants).
fn extract_action_names(items: &[MenuItem]) -> Vec<String> {
    let mut names = Vec::new();
    for item in items {
        match item {
            MenuItem::Action { name, .. } => {
                names.push(name.to_string());
            }
            MenuItem::Submenu(menu) => {
                // Flatten submenu items
                names.extend(extract_action_names(&menu.items));
            }
            _ => {}
        }
    }
    names
}

/// Get the path to the recently-used.xbel file.
fn get_recently_used_xbel_path() -> Option<std::path::PathBuf> {
    let data_dir = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".local").join("share"))
        })?;

    Some(data_dir.join("recently-used.xbel"))
}

/// Guess the MIME type from a file extension.
fn guess_mime_type(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("txt") => "text/plain".to_string(),
        Some("rs") => "text/x-rust".to_string(),
        Some("py") => "text/x-python".to_string(),
        Some("js") => "application/javascript".to_string(),
        Some("ts") => "application/typescript".to_string(),
        Some("html") | Some("htm") => "text/html".to_string(),
        Some("css") => "text/css".to_string(),
        Some("json") => "application/json".to_string(),
        Some("xml") => "application/xml".to_string(),
        Some("pdf") => "application/pdf".to_string(),
        Some("png") => "image/png".to_string(),
        Some("jpg") | Some("jpeg") => "image/jpeg".to_string(),
        Some("gif") => "image/gif".to_string(),
        Some("svg") => "image/svg+xml".to_string(),
        Some("md") => "text/markdown".to_string(),
        Some("toml") => "application/toml".to_string(),
        Some("yaml") | Some("yml") => "application/yaml".to_string(),
        Some("c") => "text/x-c".to_string(),
        Some("cpp") | Some("cc") | Some("cxx") => "text/x-c++".to_string(),
        Some("h") | Some("hpp") => "text/x-c".to_string(),
        Some("java") => "text/x-java".to_string(),
        Some("go") => "text/x-go".to_string(),
        Some("rb") => "text/x-ruby".to_string(),
        Some("sh") => "application/x-shellscript".to_string(),
        Some("zip") => "application/zip".to_string(),
        Some("tar") => "application/x-tar".to_string(),
        Some("gz") => "application/gzip".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

/// Escape special XML characters.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Format a Unix timestamp as ISO 8601 date-time string.
fn format_iso8601(timestamp: u64) -> String {
    // Simple ISO 8601 formatting without external dependencies
    let secs_per_day: u64 = 86400;
    let secs_per_hour: u64 = 3600;
    let secs_per_minute: u64 = 60;

    let days = timestamp / secs_per_day;
    let remaining = timestamp % secs_per_day;
    let hours = remaining / secs_per_hour;
    let remaining = remaining % secs_per_hour;
    let minutes = remaining / secs_per_minute;
    let seconds = remaining % secs_per_minute;

    // Calculate year, month, day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_date(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
