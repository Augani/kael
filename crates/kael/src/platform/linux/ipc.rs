use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

const MAX_UNIX_SOCKET_PATH_BYTES: usize = 100;

pub fn resolve_socket_path(app_id: &str, process_name: &str) -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/tmp".to_string());
    resolve_with_base(PathBuf::from(base), app_id, process_name)
}

fn resolve_with_base(base: PathBuf, app_id: &str, process_name: &str) -> PathBuf {
    let file_name = format!("{app_id}-{process_name}.sock");
    let candidate = base.join(&file_name);
    if base.is_absolute() && candidate.as_os_str().as_bytes().len() <= MAX_UNIX_SOCKET_PATH_BYTES {
        candidate
    } else {
        PathBuf::from("/tmp").join(file_name)
    }
}
