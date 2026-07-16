use anyhow::Result;

const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";

pub(crate) fn set_auto_launch(app_id: &str, enabled: bool) -> Result<()> {
    anyhow::ensure!(valid_app_id(app_id), "invalid auto-launch app identifier");
    let key = windows_registry::CURRENT_USER.create(RUN_KEY)?;
    if enabled {
        let exe_path = std::env::current_exe()?;
        let exe_path = exe_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("executable path is not valid Unicode"))?;
        key.set_string(app_id, &format!("\"{exe_path}\""))?;
    } else {
        let _ = key.remove_value(app_id);
    }
    Ok(())
}

pub(crate) fn is_auto_launch_enabled(app_id: &str) -> bool {
    if !valid_app_id(app_id) {
        return false;
    }
    let Ok(key) = windows_registry::CURRENT_USER.open(RUN_KEY) else {
        return false;
    };
    key.get_string(app_id).is_ok()
}

fn valid_app_id(app_id: &str) -> bool {
    !app_id.is_empty()
        && app_id.len() <= 255
        && app_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}
