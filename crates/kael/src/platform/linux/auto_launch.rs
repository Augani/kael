use anyhow::Result;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

const MAX_AUTOSTART_FILE_BYTES: u64 = 64 * 1024;
const MAX_CONFIG_PATH_BYTES: usize = 4_096;

fn xdg_config_dir() -> Option<PathBuf> {
    let path = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    (path.is_absolute() && path.as_os_str().as_encoded_bytes().len() <= MAX_CONFIG_PATH_BYTES)
        .then_some(path)
}

fn validate_app_id(app_id: &str) -> Result<()> {
    anyhow::ensure!(!app_id.is_empty(), "app id cannot be empty");
    anyhow::ensure!(app_id.len() <= 255, "app id is too long");
    anyhow::ensure!(
        app_id
            .bytes()
            .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') }),
        "app id contains unsupported characters"
    );
    Ok(())
}

fn desktop_file_path(app_id: &str) -> Option<PathBuf> {
    xdg_config_dir().map(|dir| dir.join("autostart").join(format!("{}.desktop", app_id)))
}

pub fn set_auto_launch(app_id: &str, enabled: bool) -> Result<()> {
    validate_app_id(app_id)?;

    let desktop_file = desktop_file_path(app_id)
        .ok_or_else(|| anyhow::anyhow!("Could not determine XDG config directory"))?;

    if enabled {
        if let Some(parent) = desktop_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let exe_path = std::env::current_exe()?;
        let exe_path = exe_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("executable path is not valid UTF-8"))?;
        let content = format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\n",
            app_id,
            escape_exec_arg(exe_path)
        );
        write_atomic(&desktop_file, content.as_bytes())?;
    } else {
        if let Err(error) = std::fs::remove_file(&desktop_file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(error.into());
        }
    }

    Ok(())
}

pub fn is_auto_launch_enabled(app_id: &str) -> bool {
    if validate_app_id(app_id).is_err() {
        return false;
    }

    let Some(path) = desktop_file_path(app_id) else {
        return false;
    };

    let Ok(content) = read_bounded(&path) else {
        return false;
    };

    !content.contains("X-GNOME-Autostart-enabled=false")
}

fn escape_exec_arg(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '"' | '`' | '$') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_AUTOSTART_FILE_BYTES,
        "autostart file is too large"
    );
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("autostart path has no parent"))?;
    let temp = parent.join(format!(".kael-autostart-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn read_bounded(path: &Path) -> Result<String> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    anyhow::ensure!(file.metadata()?.is_file(), "autostart entry is not a file");
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_AUTOSTART_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_AUTOSTART_FILE_BYTES,
        "autostart file is too large"
    );
    String::from_utf8(bytes).map_err(Into::into)
}
