use crate::OsInfo;

pub fn get_os_info() -> OsInfo {
    OsInfo {
        name: "linux".into(),
        version: read_os_version().into(),
        arch: std::env::consts::ARCH.into(),
        locale: read_locale().into(),
        hostname: read_hostname().into(),
    }
}

fn read_os_version() -> String {
    let Some(contents) = read_small_utf8_file("/etc/os-release", 64 * 1024) else {
        return String::new();
    };
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return value.trim_matches('"').to_string();
        }
    }
    String::new()
}

fn read_hostname() -> String {
    read_small_utf8_file("/etc/hostname", 4 * 1024)
        .map(|s| s.trim().chars().take(255).collect())
        .unwrap_or_else(|| "unknown".to_string())
}

fn read_locale() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .filter(|locale| locale.len() <= 256 && !locale.chars().any(char::is_control))
        .map(|locale| {
            locale
                .split('.')
                .next()
                .unwrap_or(&locale)
                .replace('_', "-")
        })
        .unwrap_or_else(|| "en-US".to_string())
}

fn read_small_utf8_file(path: impl AsRef<std::path::Path>, max_bytes: u64) -> Option<String> {
    use std::io::Read as _;

    let mut file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > max_bytes {
        return None;
    }
    String::from_utf8(bytes).ok()
}
