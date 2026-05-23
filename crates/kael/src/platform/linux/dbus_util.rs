pub(crate) fn parse_dbus_uint32(stdout: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(stdout).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("uint32") {
            return rest.trim().parse::<u32>().ok();
        }
    }
    None
}
