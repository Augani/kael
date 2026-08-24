use std::ffi::OsStr;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000_u32;

#[cfg(target_os = "windows")]
/// Creates a standard process command without opening a console window on Windows.
pub fn new_std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    use std::os::windows::process::CommandExt;

    let mut command = std::process::Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(target_os = "windows"))]
/// Creates a standard process command.
pub fn new_std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    std::process::Command::new(program)
}

#[cfg(target_os = "windows")]
/// Creates an asynchronous process command without opening a console window on Windows.
pub fn new_smol_command(program: impl AsRef<OsStr>) -> smol::process::Command {
    use smol::process::windows::CommandExt;

    let mut command = smol::process::Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(all(not(target_os = "windows"), not(target_arch = "wasm32")))]
/// Creates an asynchronous process command.
pub fn new_smol_command(program: impl AsRef<OsStr>) -> smol::process::Command {
    smol::process::Command::new(program)
}
