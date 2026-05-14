use std::path::PathBuf;

pub fn resolve_socket_path(app_id: &str, process_name: &str) -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(base).join(format!("{}-{}.sock", app_id, process_name))
}
