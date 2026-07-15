use std::path::{Path, PathBuf};

use crate::{DialogKind, DialogOptions, PathPromptOptions};

const MAX_DIALOG_OUTPUT_BYTES: u64 = 64 * 1024;

/// Fallback file-open dialog using zenity/kdialog when XDG Desktop Portal is unavailable.
/// Returns `None` if the user cancelled, or `Some(paths)` on success.
pub fn prompt_for_paths_fallback(options: &PathPromptOptions) -> Option<Vec<PathBuf>> {
    if let Some(result) = try_zenity_open(options) {
        return result;
    }
    if let Some(result) = try_kdialog_open(options) {
        return result;
    }
    // No dialog tool available — return None (cancelled)
    None
}

/// Fallback file-save dialog using zenity/kdialog when XDG Desktop Portal is unavailable.
/// Returns `None` if the user cancelled, or `Some(path)` on success.
pub fn prompt_for_new_path_fallback(
    directory: &Path,
    suggested_name: Option<&str>,
) -> Option<PathBuf> {
    if let Some(result) = try_zenity_save(directory, suggested_name) {
        return result;
    }
    if let Some(result) = try_kdialog_save(directory, suggested_name) {
        return result;
    }
    None
}

fn try_zenity_open(options: &PathPromptOptions) -> Option<Option<Vec<PathBuf>>> {
    let mut cmd = std::process::Command::new("zenity");
    cmd.arg("--file-selection");

    if options.directories {
        cmd.arg("--directory");
    }
    if options.multiple {
        cmd.arg("--multiple");
        cmd.args(["--separator", "\n"]);
    }
    if let Some(ref prompt) = options.prompt {
        cmd.args(["--title", prompt.as_ref()]);
    } else {
        let title = if options.directories {
            "Open Folder"
        } else {
            "Open File"
        };
        cmd.args(["--title", title]);
    }
    for filter in &options.filters {
        cmd.arg(format!("--file-filter={}", zenity_filter_arg(filter)));
    }

    let output = bounded_output(&mut cmd)?;

    if !output.status.success() {
        // User cancelled
        return Some(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<PathBuf> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();

    if paths.is_empty() {
        Some(None)
    } else {
        Some(Some(paths))
    }
}

fn try_kdialog_open(options: &PathPromptOptions) -> Option<Option<Vec<PathBuf>>> {
    let mut cmd = std::process::Command::new("kdialog");

    if options.directories {
        cmd.arg("--getexistingdirectory");
        cmd.arg(".");
    } else {
        if options.multiple {
            cmd.arg("--getopenfilename");
            cmd.arg(".");
            cmd.arg(kdialog_filter_arg(options));
            cmd.arg("--multiple");
            cmd.args(["--separate-output"]);
        } else {
            cmd.arg("--getopenfilename");
            cmd.arg(".");
            cmd.arg(kdialog_filter_arg(options));
        }
    }

    let title = if let Some(ref prompt) = options.prompt {
        prompt.to_string()
    } else if options.directories {
        "Open Folder".to_string()
    } else {
        "Open File".to_string()
    };
    cmd.args(["--title", &title]);

    let output = bounded_output(&mut cmd)?;

    if !output.status.success() {
        return Some(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<PathBuf> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();

    if paths.is_empty() {
        Some(None)
    } else {
        Some(Some(paths))
    }
}

fn zenity_filter_arg(filter: &crate::FileDialogFilter) -> String {
    let patterns = filter
        .extensions
        .iter()
        .map(|extension| format!("*.{}", extension.as_ref()))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{} | {}", filter.name.as_ref(), patterns)
}

fn kdialog_filter_arg(options: &PathPromptOptions) -> String {
    if options.filters.is_empty() {
        return "*".to_string();
    }

    options
        .filters
        .iter()
        .map(|filter| {
            let patterns = filter
                .extensions
                .iter()
                .map(|extension| format!("*.{}", extension.as_ref()))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} ({})", filter.name.as_ref(), patterns)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn try_zenity_save(directory: &Path, suggested_name: Option<&str>) -> Option<Option<PathBuf>> {
    let mut cmd = std::process::Command::new("zenity");
    cmd.args(["--file-selection", "--save", "--confirm-overwrite"]);
    cmd.args(["--title", "Save File"]);

    // Build the initial filename path
    if let Some(name) = suggested_name {
        let full_path = directory.join(name);
        cmd.args(["--filename", &full_path.to_string_lossy()]);
    } else {
        let dir_str = format!("{}/", directory.display());
        cmd.args(["--filename", &dir_str]);
    }

    let output = bounded_output(&mut cmd)?;

    if !output.status.success() {
        return Some(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let path = stdout.trim();
    if path.is_empty() {
        Some(None)
    } else {
        Some(Some(PathBuf::from(path)))
    }
}

fn try_kdialog_save(directory: &Path, suggested_name: Option<&str>) -> Option<Option<PathBuf>> {
    let mut cmd = std::process::Command::new("kdialog");
    cmd.arg("--getsavefilename");

    if let Some(name) = suggested_name {
        let full_path = directory.join(name);
        cmd.arg(full_path.to_string_lossy().as_ref());
    } else {
        let dir_str = format!("{}/", directory.display());
        cmd.arg(&dir_str);
    }

    cmd.args(["--title", "Save File"]);

    let output = bounded_output(&mut cmd)?;

    if !output.status.success() {
        return Some(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let path = stdout.trim();
    if path.is_empty() {
        Some(None)
    } else {
        Some(Some(PathBuf::from(path)))
    }
}

pub fn show_dialog(options: &DialogOptions) -> usize {
    if !valid_dialog_options(options) {
        return 0;
    }
    if let Some(idx) = try_zenity(options) {
        return idx;
    }
    if let Some(idx) = try_kdialog(options) {
        return idx;
    }
    0
}

fn valid_dialog_options(options: &DialogOptions) -> bool {
    !options.title.is_empty()
        && options.title.len() <= 256
        && options.message.len() <= 2_048
        && options
            .detail
            .as_ref()
            .is_none_or(|detail| detail.len() <= 4_096)
        && (1..=6).contains(&options.buttons.len())
        && options.buttons.iter().all(|button| {
            !button.is_empty() && button.len() <= 128 && !button.chars().any(char::is_control)
        })
}

fn try_zenity(options: &DialogOptions) -> Option<usize> {
    let message = build_message(options);
    let button_count = options.buttons.len();

    if button_count <= 1 {
        let zenity_type = match options.kind {
            DialogKind::Error => "--error",
            DialogKind::Warning => "--warning",
            DialogKind::Info => "--info",
        };
        let _ = std::process::Command::new("zenity")
            .args([zenity_type, "--title", &options.title, "--text", &message])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?;
        return Some(0);
    }

    let mut cmd = std::process::Command::new("zenity");
    cmd.args(["--question", "--title", &options.title, "--text", &message]);

    cmd.args(["--ok-label", &options.buttons[0]]);
    cmd.args(["--cancel-label", &options.buttons[1]]);

    for button in options.buttons.iter().skip(2) {
        cmd.args(["--extra-button", button]);
    }

    let output = bounded_output(&mut cmd)?;

    if output.status.success() {
        return Some(0);
    }

    if button_count > 2 {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout_trimmed = stdout.trim();
        for (idx, button) in options.buttons.iter().enumerate().skip(2) {
            if stdout_trimmed == button.as_ref() {
                return Some(idx);
            }
        }
    }

    Some(1)
}

fn try_kdialog(options: &DialogOptions) -> Option<usize> {
    let message = build_message(options);
    let button_count = options.buttons.len();

    if button_count <= 1 {
        let kdialog_type = match options.kind {
            DialogKind::Error => "--error",
            DialogKind::Warning => "--sorry",
            DialogKind::Info => "--msgbox",
        };
        let _ = std::process::Command::new("kdialog")
            .args([kdialog_type, &message, "--title", &options.title])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?;
        return Some(0);
    }

    let mut args = vec!["--warningyesno".to_string(), message];
    if button_count >= 3 {
        args[0] = "--warningyesnocancel".to_string();
    }

    args.push("--title".to_string());
    args.push(options.title.to_string());

    if button_count >= 1 {
        args.push("--yes-label".to_string());
        args.push(options.buttons[0].to_string());
    }
    if button_count >= 2 {
        args.push("--no-label".to_string());
        args.push(options.buttons[1].to_string());
    }
    if button_count >= 3 {
        args.push("--cancel-label".to_string());
        args.push(options.buttons[2].to_string());
    }

    let mut command = std::process::Command::new("kdialog");
    command.args(&args);
    let output = bounded_output(&mut command)?;

    match output.status.code() {
        Some(0) => Some(0),
        Some(1) => Some(1),
        Some(2) => Some(2),
        _ => Some(0),
    }
}

fn build_message(options: &DialogOptions) -> String {
    match &options.detail {
        Some(detail) if !options.message.is_empty() => {
            format!("{}\n\n{}", options.message, detail)
        }
        Some(detail) => detail.to_string(),
        None => options.message.to_string(),
    }
}

fn bounded_output(command: &mut std::process::Command) -> Option<std::process::Output> {
    use std::io::Read as _;

    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().ok()?;
    let pid = child.id();
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::Builder::new()
        .name("linux-dialog-output".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .by_ref()
                .take(MAX_DIALOG_OUTPUT_BYTES + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() as u64 > MAX_DIALOG_OUTPUT_BYTES {
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                return None;
            }
            Some(bytes)
        })
        .ok();
    let Some(reader) = reader else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let status = child.wait().ok()?;
    let stdout = reader.join().ok()??;
    Some(std::process::Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}
