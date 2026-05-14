pub fn pipe_name(app_id: &str, process_name: &str) -> String {
    format!("{}-{}", app_id, process_name)
}
