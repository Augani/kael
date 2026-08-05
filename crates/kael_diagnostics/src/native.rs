//! Native (non-panic) crash capture.
//!
//! Rust's `std::panic` hook only fires for panics. Hardware faults
//! (SIGSEGV/SIGBUS/SIGILL/SIGFPE), aborts (SIGABRT), and foreign-code crashes
//! never unwind through it. This module installs OS-level handlers that capture
//! a minimal, async-signal-safe crash record so the next launch can assemble
//! and submit a report.
//!
//! Signal-safety contract: everything reachable from a signal/exception handler
//! (the functions in the `imp` submodules below) must avoid heap allocation,
//! locks, and non-async-signal-safe libc calls. All human-readable context is
//! pre-rendered to disk at install time; the handler only appends fixed-shape
//! `key=value` lines to a file descriptor opened ahead of the crash.

use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

const META_SUFFIX: &str = ".crashmeta.json";
const DUMP_SUFFIX: &str = ".crashdump";

static INSTALLED: AtomicBool = AtomicBool::new(false);
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
const MAX_META_BYTES: u64 = 1024 * 1024;
const MAX_DUMP_BYTES: u64 = 64 * 1024;
const MAX_SESSION_ID_BYTES: usize = 128;
const MAX_PENDING_NATIVE_CRASHES: usize = 256;

/// Pre-crash context captured at [`install`] time and persisted alongside the
/// dump so the next launch can reconstruct a full report without doing any work
/// in the signal handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeContext {
    /// Unique identifier for this process session.
    pub session_id: String,
    /// Application release/version string at install time.
    pub app_version: Option<String>,
    /// Deployment environment at install time.
    pub environment: Option<String>,
    /// Operating system name.
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// Process identifier at install time.
    pub pid: u32,
    /// Milliseconds since the Unix epoch when the session started.
    pub started_at_ms: u128,
}

/// A native crash signal record decoded from a dump file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeSignal {
    /// The platform signal/exception number (e.g. SIGSEGV) or Windows code.
    pub signal: i64,
    /// The platform-specific signal code, if captured.
    pub code: Option<i64>,
    /// The faulting address, if the platform reported one.
    pub fault_address: Option<u64>,
    /// Captured instruction-pointer / frame return addresses, in hex.
    pub frames: Vec<String>,
}

/// Outcome of decoding a single pending native crash on the next launch.
#[derive(Debug, Clone)]
pub struct PendingNativeCrash {
    /// The pre-crash context for the session that crashed.
    pub context: NativeContext,
    /// The decoded signal record, present only when a native dump was written.
    pub signal: Option<NativeSignal>,
    /// Path to the meta file (consumed when the crash is cleared).
    pub meta_path: PathBuf,
    /// Path to the dump file, if one exists.
    pub dump_path: Option<PathBuf>,
}

impl PendingNativeCrash {
    /// Whether a native dump (signal record) was actually captured. When false,
    /// the prior run ended uncleanly without a handler firing (e.g. SIGKILL or
    /// power loss).
    pub fn has_native_record(&self) -> bool {
        self.signal.is_some()
    }

    /// A short human-readable summary suitable for a crash report message.
    pub fn summary(&self) -> String {
        match &self.signal {
            Some(signal) => {
                let name = signal_name(signal.signal);
                match signal.fault_address {
                    Some(address) => format!("native crash: {name} at {address:#018x}"),
                    None => format!("native crash: {name}"),
                }
            }
            None => "unclean exit: previous run terminated without a clean shutdown".to_string(),
        }
    }

    /// A multi-line backtrace string built from the captured frame addresses.
    pub fn backtrace_text(&self) -> String {
        match &self.signal {
            Some(signal) if !signal.frames.is_empty() => signal
                .frames
                .iter()
                .enumerate()
                .map(|(index, frame)| format!("{index:>3}: {frame}"))
                .collect::<Vec<_>>()
                .join("\n"),
            Some(_) => "no frames captured".to_string(),
            None => String::new(),
        }
    }
}

fn meta_path(reports_dir: &Path, session_id: &str) -> PathBuf {
    reports_dir.join(format!("{session_id}{META_SUFFIX}"))
}

fn dump_path(reports_dir: &Path, session_id: &str) -> PathBuf {
    reports_dir.join(format!("{session_id}{DUMP_SUFFIX}"))
}

/// Generates a process-unique session identifier without external crates.
pub fn new_session_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let pid = std::process::id();
    let sequence = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:032x}-{pid:08x}-{sequence:016x}")
}

fn write_context(reports_dir: &Path, context: &NativeContext) -> Result<()> {
    validate_session_id(&context.session_id)?;
    crate::crash::prepare_reports_dir(reports_dir)?;
    let path = meta_path(reports_dir, &context.session_id);
    let json = serde_json::to_string_pretty(context).context("failed to serialize crash meta")?;
    if json.len() as u64 > MAX_META_BYTES {
        return Err(anyhow::anyhow!(
            "serialized native crash metadata exceeds {MAX_META_BYTES} byte limit"
        ));
    }
    let temp = path.with_extension("json.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .with_context(|| format!("failed to create crash meta: {}", temp.display()))?;
    use std::io::Write as _;
    let write_result = file
        .write_all(json.as_bytes())
        .with_context(|| format!("failed to write crash meta: {}", temp.display()))
        .and_then(|()| {
            file.sync_all()
                .with_context(|| format!("failed to sync crash meta: {}", temp.display()))
        });
    if let Err(error) = write_result {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    drop(file);
    if let Err(error) = fs::rename(&temp, &path).with_context(|| {
        format!(
            "failed to finalize crash meta from {} to {}",
            temp.display(),
            path.display()
        )
    }) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    crate::crash::sync_parent_dir(reports_dir)?;
    Ok(())
}

/// Returns the canonical signal name for a captured signal number.
pub fn signal_name(signal: i64) -> &'static str {
    #[cfg(unix)]
    {
        match signal as i32 {
            libc::SIGSEGV => "SIGSEGV",
            libc::SIGBUS => "SIGBUS",
            libc::SIGABRT => "SIGABRT",
            libc::SIGILL => "SIGILL",
            libc::SIGFPE => "SIGFPE",
            libc::SIGTRAP => "SIGTRAP",
            _ => "signal",
        }
    }
    #[cfg(windows)]
    {
        match signal as u32 {
            0xC000_0005 => "EXCEPTION_ACCESS_VIOLATION",
            0xC000_001D => "EXCEPTION_ILLEGAL_INSTRUCTION",
            0xC000_0094 => "EXCEPTION_INT_DIVIDE_BY_ZERO",
            0xC000_0096 => "EXCEPTION_PRIV_INSTRUCTION",
            0x8000_0003 => "EXCEPTION_BREAKPOINT",
            _ => "exception",
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = signal;
        "signal"
    }
}

/// Installs native crash handlers, persisting pre-crash context first.
///
/// Returns `false` (without re-installing) if handlers are already installed in
/// this process. On unsupported targets this persists context but installs no
/// handler, returning `true`.
pub fn install(reports_dir: &Path, context: NativeContext) -> Result<bool> {
    if INSTALLED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(false);
    }

    let result = (|| {
        write_context(reports_dir, &context)?;
        let dump = dump_path(reports_dir, &context.session_id);

        #[cfg(unix)]
        {
            imp::install_unix(&dump, &context.session_id)?;
        }
        #[cfg(windows)]
        {
            imp::install_windows(&dump, &context.session_id)?;
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = dump;
        }
        Ok::<(), anyhow::Error>(())
    })();
    if let Err(error) = result {
        INSTALLED.store(false, Ordering::SeqCst);
        return Err(error);
    }
    Ok(true)
}

/// Decodes pending native crashes from `reports_dir`. Each session that has a
/// leftover meta file is reported: a present, non-empty dump yields a decoded
/// signal record; an absent/empty dump indicates an unclean exit with no native
/// data.
pub fn pending_crashes(reports_dir: &Path) -> Result<Vec<PendingNativeCrash>> {
    let mut crashes = Vec::new();
    if !reports_dir.exists() {
        return Ok(crashes);
    }
    let directory_metadata = fs::symlink_metadata(reports_dir).with_context(|| {
        format!(
            "failed to inspect native crash directory: {}",
            reports_dir.display()
        )
    })?;
    if !directory_metadata.file_type().is_dir() {
        return Err(anyhow::anyhow!(
            "native crash path must be a real directory: {}",
            reports_dir.display()
        ));
    }

    let current = current_session_id();

    for entry in fs::read_dir(reports_dir).with_context(|| {
        format!(
            "failed to read native crash directory: {}",
            reports_dir.display()
        )
    })? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(session_id) = name.strip_suffix(META_SUFFIX) else {
            continue;
        };
        if current.as_deref() == Some(session_id) {
            continue;
        }

        if validate_session_id(session_id).is_err() {
            continue;
        }
        let Ok(bytes) = read_bounded_regular_file(&path, MAX_META_BYTES) else {
            continue;
        };
        let Ok(json) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let context: NativeContext = match serde_json::from_str(json) {
            Ok(context) => context,
            Err(_) => continue,
        };
        if context.session_id != session_id {
            continue;
        }

        let dump = dump_path(reports_dir, session_id);
        let (signal, dump_path_out) = match read_bounded_regular_file(&dump, MAX_DUMP_BYTES) {
            Ok(bytes) if bytes.is_empty() => (None, None),
            Ok(bytes) => (decode_dump(&bytes), Some(dump)),
            _ => (None, None),
        };

        crashes.push(PendingNativeCrash {
            context,
            signal,
            meta_path: path,
            dump_path: dump_path_out,
        });
        if crashes.len() == MAX_PENDING_NATIVE_CRASHES {
            break;
        }
    }

    crashes.sort_by_key(|crash| crash.context.started_at_ms);
    Ok(crashes)
}

fn read_bounded_regular_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).with_context(|| {
        format!(
            "failed to inspect native crash artifact: {}",
            path.display()
        )
    })?;
    if !metadata.file_type().is_file() || metadata.len() > max_bytes {
        return Err(anyhow::anyhow!(
            "native crash artifact must be a regular file of at most {max_bytes} bytes: {}",
            path.display()
        ));
    }

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .with_context(|| format!("failed to open native crash artifact: {}", path.display()))?;
    let opened_metadata = file.metadata().with_context(|| {
        format!(
            "failed to inspect native crash artifact: {}",
            path.display()
        )
    })?;
    if !opened_metadata.is_file() || opened_metadata.len() > max_bytes {
        return Err(anyhow::anyhow!(
            "native crash artifact changed while opening: {}",
            path.display()
        ));
    }
    let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read native crash artifact: {}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(anyhow::anyhow!(
            "native crash artifact exceeds {max_bytes} byte limit: {}",
            path.display()
        ));
    }
    Ok(bytes)
}

/// Removes the meta and dump artifacts for a decoded crash once reported.
pub fn clear_crash(crash: &PendingNativeCrash) {
    let _ = fs::remove_file(&crash.meta_path);
    if let Some(dump) = &crash.dump_path {
        let _ = fs::remove_file(dump);
    }
}

/// Marks the current session as a clean exit, removing its (empty) artifacts so
/// the next launch does not treat this run as an unclean exit.
pub fn mark_clean_exit(reports_dir: &Path) {
    let Some(session_id) = current_session_id() else {
        return;
    };
    let _ = fs::remove_file(meta_path(reports_dir, &session_id));
    let _ = fs::remove_file(dump_path(reports_dir, &session_id));
}

fn current_session_id() -> Option<String> {
    imp::current_session_id()
}

fn decode_dump(bytes: &[u8]) -> Option<NativeSignal> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut signal = NativeSignal::default();
    let mut saw_signal = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "signal" => {
                signal.signal = parse_int(value)?;
                saw_signal = true;
            }
            "code" => signal.code = parse_int(value),
            "address" => signal.fault_address = parse_uint(value),
            "frame" if signal.frames.len() < 64 => signal.frames.push(value.trim().to_string()),
            _ => {}
        }
    }
    saw_signal.then_some(signal)
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || session_id.len() > MAX_SESSION_ID_BYTES
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(anyhow::anyhow!("invalid native crash session identifier"));
    }
    Ok(())
}

fn parse_int(value: &str) -> Option<i64> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}

fn parse_uint(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}

#[cfg(unix)]
mod imp {
    use std::{
        os::fd::IntoRawFd as _,
        path::Path,
        sync::{
            Mutex,
            atomic::{AtomicI32, Ordering},
        },
    };

    use anyhow::{Context as _, Result, anyhow};

    static DUMP_FD: AtomicI32 = AtomicI32::new(-1);
    static SESSION_ID: Mutex<Option<String>> = Mutex::new(None);

    const SIGNALS: [i32; 5] = [
        libc::SIGSEGV,
        libc::SIGBUS,
        libc::SIGABRT,
        libc::SIGILL,
        libc::SIGFPE,
    ];

    const MAX_FRAMES: usize = 64;

    #[cfg(target_arch = "x86_64")]
    const RA_OFFSET: usize = 8;
    #[cfg(target_arch = "aarch64")]
    const RA_OFFSET: usize = 8;
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    const RA_OFFSET: usize = std::mem::size_of::<usize>();

    pub(super) fn current_session_id() -> Option<String> {
        SESSION_ID
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn install_unix(dump: &Path, session_id: &str) -> Result<()> {
        *SESSION_ID
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(session_id.to_string());

        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
        let file = options
            .open(dump)
            .with_context(|| format!("failed to pre-open crash dump: {}", dump.display()))?;
        let fd = file.into_raw_fd();
        DUMP_FD.store(fd, Ordering::SeqCst);

        for signal in SIGNALS {
            install_one(signal)?;
        }
        Ok(())
    }

    fn install_one(signal: i32) -> Result<()> {
        // SAFETY: a zeroed sigaction is a valid empty handler descriptor; we set
        // the fields we depend on before installing it.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = handler as *const () as usize;
        action.sa_flags = libc::SA_SIGINFO | libc::SA_RESETHAND;
        // SAFETY: sigemptyset on a stack-allocated, owned set is well defined.
        unsafe {
            libc::sigemptyset(&mut action.sa_mask);
        }
        // SAFETY: registering a valid handler for a real signal number.
        let result = unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) };
        if result != 0 {
            return Err(anyhow!("sigaction failed for signal {signal}"));
        }
        Ok(())
    }

    // Async-signal-safe from here down: no allocation, no locks, no formatting.
    extern "C" fn handler(
        signal: libc::c_int,
        info: *mut libc::siginfo_t,
        _context: *mut libc::c_void,
    ) {
        let fd = DUMP_FD.load(Ordering::SeqCst);
        if fd >= 0 {
            write_record(fd, signal, info);
        }
        // SA_RESETHAND already reset this signal to SIG_DFL; re-raising delivers
        // it fatally so the OS still produces its own crash and exit status.
        // SAFETY: raise(2) is async-signal-safe.
        unsafe {
            libc::raise(signal);
        }
    }

    fn write_record(fd: i32, signal: libc::c_int, info: *mut libc::siginfo_t) {
        write_kv_int(fd, b"signal=", signal as i64);

        if !info.is_null() {
            // SAFETY: the kernel hands us a valid siginfo_t for SA_SIGINFO.
            let code = unsafe { (*info).si_code } as i64;
            write_kv_int(fd, b"code=", code);
            // SAFETY: si_addr() reads the fault address union member, valid for
            // the memory-fault signals we register.
            let address = unsafe { (*info).si_addr() } as usize as u64;
            write_kv_hex(fd, b"address=", address);
        }

        write_backtrace(fd);
    }

    fn write_backtrace(fd: i32) {
        let mut frame = current_frame_pointer();
        let mut count = 0usize;
        while count < MAX_FRAMES {
            if frame == 0 || frame & 0x7 != 0 {
                break;
            }
            // SAFETY: a frame-pointer chain stores [saved fp][return addr]. We
            // only dereference 8-byte-aligned, non-null links and stop on the
            // first invalid one. A read may still fault on a corrupt stack; that
            // re-enters the now-SIG_DFL handler and the OS terminates us.
            let return_address = unsafe { *((frame + RA_OFFSET) as *const usize) };
            let next = unsafe { *(frame as *const usize) };
            if return_address != 0 {
                write_kv_hex(fd, b"frame=", return_address as u64);
            }
            if next <= frame {
                break;
            }
            frame = next;
            count += 1;
        }
    }

    #[inline(always)]
    fn current_frame_pointer() -> usize {
        #[allow(unused_assignments)]
        let mut frame: usize = 0;
        #[cfg(target_arch = "x86_64")]
        // SAFETY: reads the frame-base register; no memory access, no side effects.
        unsafe {
            std::arch::asm!("mov {}, rbp", out(reg) frame, options(nomem, nostack, preserves_flags));
        }
        #[cfg(target_arch = "aarch64")]
        // SAFETY: reads the frame-base register; no memory access, no side effects.
        unsafe {
            std::arch::asm!("mov {}, x29", out(reg) frame, options(nomem, nostack, preserves_flags));
        }
        frame
    }

    fn write_kv_int(fd: i32, key: &[u8], value: i64) {
        write_all(fd, key);
        let mut buffer = [0u8; 24];
        let bytes = render_int(value, &mut buffer);
        write_all(fd, bytes);
        write_all(fd, b"\n");
    }

    fn write_kv_hex(fd: i32, key: &[u8], value: u64) {
        write_all(fd, key);
        let mut buffer = [0u8; 18];
        let bytes = render_hex(value, &mut buffer);
        write_all(fd, bytes);
        write_all(fd, b"\n");
    }

    fn write_all(fd: i32, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            // SAFETY: write(2) is async-signal-safe; fd is owned for the process
            // lifetime. A short write advances the slice; EINTR retries.
            let written =
                unsafe { libc::write(fd, bytes.as_ptr() as *const libc::c_void, bytes.len()) };
            if written <= 0 {
                let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if errno == libc::EINTR {
                    continue;
                }
                break;
            }
            bytes = &bytes[written as usize..];
        }
    }

    pub(super) fn render_int(value: i64, buffer: &mut [u8; 24]) -> &[u8] {
        let negative = value < 0;
        let mut magnitude = value.unsigned_abs();
        let mut index = buffer.len();
        loop {
            index -= 1;
            buffer[index] = b'0' + (magnitude % 10) as u8;
            magnitude /= 10;
            if magnitude == 0 {
                break;
            }
        }
        if negative {
            index -= 1;
            buffer[index] = b'-';
        }
        &buffer[index..]
    }

    pub(super) fn render_hex(value: u64, buffer: &mut [u8; 18]) -> &[u8] {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut index = buffer.len();
        let mut remaining = value;
        loop {
            index -= 1;
            buffer[index] = DIGITS[(remaining & 0xf) as usize];
            remaining >>= 4;
            if remaining == 0 {
                break;
            }
        }
        index -= 1;
        buffer[index] = b'x';
        index -= 1;
        buffer[index] = b'0';
        &buffer[index..]
    }
}

#[cfg(windows)]
mod imp {
    use std::{
        os::windows::io::IntoRawHandle as _,
        path::Path,
        sync::{
            Mutex,
            atomic::{AtomicIsize, Ordering},
        },
    };

    use anyhow::{Context as _, Result};
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::WriteFile,
        System::Diagnostics::Debug::{
            EXCEPTION_POINTERS, RtlCaptureStackBackTrace, SetUnhandledExceptionFilter,
        },
    };

    static DUMP_HANDLE: AtomicIsize = AtomicIsize::new(0);
    static SESSION_ID: Mutex<Option<String>> = Mutex::new(None);
    const MAX_FRAMES: usize = 62;
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    pub(super) fn current_session_id() -> Option<String> {
        SESSION_ID
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn install_windows(dump: &Path, session_id: &str) -> Result<()> {
        *SESSION_ID
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(session_id.to_string());

        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(dump)
            .with_context(|| format!("failed to pre-open crash dump: {}", dump.display()))?;
        let handle = file.into_raw_handle() as isize;
        DUMP_HANDLE.store(handle, Ordering::SeqCst);

        // SAFETY: installing a process-wide unhandled-exception filter.
        unsafe {
            SetUnhandledExceptionFilter(Some(filter));
        }
        Ok(())
    }

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        let handle = DUMP_HANDLE.load(Ordering::SeqCst);
        if handle != 0 && !info.is_null() {
            // SAFETY: the OS hands us valid EXCEPTION_POINTERS.
            let record = unsafe { (*info).ExceptionRecord };
            if !record.is_null() {
                let code = unsafe { (*record).ExceptionCode } as i64;
                let address = unsafe { (*record).ExceptionAddress } as u64;
                write_kv_int(handle as HANDLE, b"signal=", code);
                write_kv_hex(handle as HANDLE, b"address=", address);
                write_backtrace(handle as HANDLE);
            }
        }
        EXCEPTION_CONTINUE_SEARCH
    }

    fn write_backtrace(handle: HANDLE) {
        let mut frames = [std::ptr::null_mut::<core::ffi::c_void>(); MAX_FRAMES];
        // SAFETY: RtlCaptureStackBackTrace fills our fixed buffer; it is callable
        // from an exception filter and performs no heap allocation.
        let captured = unsafe {
            RtlCaptureStackBackTrace(
                0,
                MAX_FRAMES as u32,
                frames.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        };
        for frame in frames.iter().take(captured as usize) {
            write_kv_hex(handle, b"frame=", *frame as u64);
        }
    }

    fn write_kv_int(handle: HANDLE, key: &[u8], value: i64) {
        write_all(handle, key);
        let mut buffer = [0u8; 24];
        let bytes = render_int(value, &mut buffer);
        write_all(handle, bytes);
        write_all(handle, b"\n");
    }

    fn write_kv_hex(handle: HANDLE, key: &[u8], value: u64) {
        write_all(handle, key);
        let mut buffer = [0u8; 18];
        let bytes = render_hex(value, &mut buffer);
        write_all(handle, bytes);
        write_all(handle, b"\n");
    }

    fn write_all(handle: HANDLE, bytes: &[u8]) {
        let mut written = 0u32;
        // SAFETY: WriteFile to an owned handle; bytes outlives the call.
        unsafe {
            WriteFile(
                handle,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            );
        }
    }

    fn render_int(value: i64, buffer: &mut [u8; 24]) -> &[u8] {
        let negative = value < 0;
        let mut magnitude = value.unsigned_abs();
        let mut index = buffer.len();
        loop {
            index -= 1;
            buffer[index] = b'0' + (magnitude % 10) as u8;
            magnitude /= 10;
            if magnitude == 0 {
                break;
            }
        }
        if negative {
            index -= 1;
            buffer[index] = b'-';
        }
        &buffer[index..]
    }

    fn render_hex(value: u64, buffer: &mut [u8; 18]) -> &[u8] {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut index = buffer.len();
        let mut remaining = value;
        loop {
            index -= 1;
            buffer[index] = DIGITS[(remaining & 0xf) as usize];
            remaining >>= 4;
            if remaining == 0 {
                break;
            }
        }
        index -= 1;
        buffer[index] = b'x';
        index -= 1;
        buffer[index] = b'0';
        &buffer[index..]
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub(super) fn current_session_id() -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_META_BYTES, NativeContext, NativeSignal, PendingNativeCrash, decode_dump,
        new_session_id, parse_int, parse_uint, pending_crashes, signal_name, write_context,
    };

    fn context(session_id: &str) -> NativeContext {
        NativeContext {
            session_id: session_id.to_string(),
            app_version: Some("1.0.0".to_string()),
            environment: Some("test".to_string()),
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
            pid: 1,
            started_at_ms: 0,
        }
    }

    #[test]
    fn session_ids_are_unique() {
        let first = new_session_id();
        let second = new_session_id();
        assert_ne!(first, second);
    }

    #[test]
    fn parses_hex_and_decimal() {
        assert_eq!(parse_int("11"), Some(11));
        assert_eq!(parse_int("0xb"), Some(11));
        assert_eq!(parse_uint("0xdeadbeef"), Some(0xdead_beef));
    }

    #[test]
    fn decodes_signal_record() {
        let dump = b"signal=11\ncode=1\naddress=0x10\nframe=0x4001\nframe=0x4002\n";
        let signal = decode_dump(dump).expect("record should decode");
        assert_eq!(signal.signal, 11);
        assert_eq!(signal.code, Some(1));
        assert_eq!(signal.fault_address, Some(0x10));
        assert_eq!(signal.frames, vec!["0x4001", "0x4002"]);
    }

    #[test]
    fn empty_dump_decodes_to_none() {
        assert!(decode_dump(b"code=1\n").is_none());
    }

    #[test]
    fn summary_describes_signal() {
        let crash = PendingNativeCrash {
            context: context("s"),
            signal: Some(NativeSignal {
                signal: 11,
                code: Some(1),
                fault_address: Some(0x10),
                frames: vec!["0x4001".to_string()],
            }),
            meta_path: "meta".into(),
            dump_path: Some("dump".into()),
        };
        assert!(crash.has_native_record());
        assert!(crash.summary().contains("SIGSEGV"));
        assert!(crash.backtrace_text().contains("0x4001"));
    }

    #[test]
    fn signal_names_are_known() {
        #[cfg(unix)]
        assert_eq!(signal_name(libc::SIGSEGV as i64), "SIGSEGV");
        #[cfg(not(unix))]
        let _ = signal_name(11);
    }

    #[test]
    fn rejects_oversized_native_context_before_writing() {
        let directory = tempfile::tempdir().unwrap();
        let mut context = context("oversized");
        context.environment = Some("x".repeat(MAX_META_BYTES as usize));

        assert!(write_context(directory.path(), &context).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn pending_crashes_ignore_symlinked_metadata() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        serde_json::to_writer(outside.as_file(), &context("linked")).unwrap();
        symlink(
            outside.path(),
            directory.path().join("linked.crashmeta.json"),
        )
        .unwrap();

        assert!(pending_crashes(directory.path()).unwrap().is_empty());
    }
}
