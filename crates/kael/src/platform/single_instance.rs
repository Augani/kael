/// Cross-platform single instance enforcement for applications.
///
/// Uses Unix domain sockets on macOS/Linux and named mutexes on Windows
/// to ensure only one instance of an application runs at a time.
use anyhow::Result;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

/// Error returned when another instance of the application is already running.
#[derive(Debug)]
pub struct AlreadyRunning;

impl std::fmt::Display for AlreadyRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Another instance is already running")
    }
}

impl std::error::Error for AlreadyRunning {}

/// A guard that enforces single-instance behavior for an application.
///
/// When acquired successfully, this struct holds a platform-specific lock
/// that prevents other instances from starting. The lock is released when
/// this struct is dropped.
pub struct SingleInstance {
    app_id: String,
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    _listener: std::os::unix::net::UnixListener,
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    _socket_path: PathBuf,
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    _lock_file: File,
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    activation_listener_started: AtomicBool,
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    activation_stop: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    _mutex: WindowsMutexHandle,
}

#[cfg(target_os = "windows")]
struct WindowsMutexHandle {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(target_os = "windows")]
impl Drop for WindowsMutexHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
fn socket_path(app_id: &str) -> Result<PathBuf> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let base_dir = std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/tmp".to_string());
    let effective_user = unsafe { libc::geteuid() };
    let dir = PathBuf::from(base_dir).join(format!("kael-{effective_user}"));
    match std::fs::create_dir(&dir) {
        Ok(()) => {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(anyhow::anyhow!(
                "creating single-instance runtime directory {dir:?}: {error}"
            ));
        }
    }
    let metadata = std::fs::symlink_metadata(&dir).map_err(|error| {
        anyhow::anyhow!("inspecting single-instance runtime directory {dir:?}: {error}")
    })?;
    anyhow::ensure!(
        metadata.file_type().is_dir() && !metadata.file_type().is_symlink(),
        "single-instance runtime path is not a real directory: {dir:?}"
    );
    anyhow::ensure!(
        metadata.uid() == effective_user,
        "single-instance runtime directory is owned by another user: {dir:?}"
    );
    if metadata.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(
            |error| anyhow::anyhow!("securing single-instance runtime directory {dir:?}: {error}"),
        )?;
    }
    Ok(dir.join(format!("{app_id}.sock")))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
fn lock_path(socket_path: &Path) -> PathBuf {
    socket_path.with_extension("lock")
}

fn validate_single_instance_app_id(app_id: &str) -> Result<()> {
    anyhow::ensure!(
        !app_id.trim().is_empty(),
        "single-instance app id cannot be empty"
    );
    anyhow::ensure!(
        app_id == app_id.trim(),
        "single-instance app id cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        app_id.len() <= 128,
        "single-instance app id cannot be longer than 128 bytes"
    );
    anyhow::ensure!(
        !app_id.chars().any(char::is_control),
        "single-instance app id cannot contain control characters"
    );
    anyhow::ensure!(
        !app_id.contains('/') && !app_id.contains('\\'),
        "single-instance app id cannot contain path separators"
    );
    Ok(())
}

impl SingleInstance {
    /// Return the application identifier used for the single-instance lock.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Attempt to acquire the single-instance lock for the given application ID.
    ///
    /// Returns `Ok(SingleInstance)` if this is the first instance, or
    /// `Err(AlreadyRunning)` if another instance already holds the lock.
    pub fn acquire(app_id: &str) -> std::result::Result<Self, AlreadyRunning> {
        Self::try_acquire(app_id)
            .ok()
            .flatten()
            .ok_or(AlreadyRunning)
    }

    /// Attempt to acquire the single-instance lock while preserving operating-system errors.
    ///
    /// `Ok(Some(_))` means this process is primary, `Ok(None)` means another process owns the
    /// lock, and `Err` reports an actual lock, socket, or platform initialization failure.
    pub fn try_acquire(app_id: &str) -> Result<Option<Self>> {
        validate_single_instance_app_id(app_id)?;
        Self::platform_try_acquire(app_id)
    }

    /// Register a callback to be invoked when another instance attempts to start
    /// and sends an activation message.
    ///
    /// On Unix platforms, this spawns a background thread that listens for
    /// incoming connections on the Unix domain socket.
    pub fn on_activate(&self, callback: Box<dyn Fn() + Send + 'static>) {
        self.platform_on_activate(callback);
    }
}

#[cfg(target_arch = "wasm32")]
impl SingleInstance {
    fn platform_try_acquire(app_id: &str) -> Result<Option<Self>> {
        Ok(Some(Self {
            app_id: app_id.to_string(),
        }))
    }

    fn platform_on_activate(&self, _callback: Box<dyn Fn() + Send + 'static>) {}
}

/// Send an activation message to an already-running instance of the application.
///
/// This is typically called after `SingleInstance::acquire` returns `Err(AlreadyRunning)`
/// to signal the existing instance to come to the foreground.
pub fn send_activate_to_existing(app_id: &str) -> Result<()> {
    validate_single_instance_app_id(app_id)?;
    platform_send_activate(app_id)
}

/// Outcome of a single-instance launch attempt.
pub enum SingleInstanceLaunch {
    /// This process is the primary application instance.
    Primary(SingleInstance),
    /// Another process is already running.
    Duplicate {
        /// Application identifier used for the single-instance lock.
        app_id: String,
        /// Whether this duplicate launch notified the existing instance.
        notified: bool,
    },
}

impl SingleInstanceLaunch {
    /// Return the application identifier used for the single-instance lock.
    pub fn app_id(&self) -> &str {
        match self {
            Self::Primary(instance) => instance.app_id(),
            Self::Duplicate { app_id, .. } => app_id,
        }
    }

    /// Return true when this process owns the single-instance lock.
    pub fn is_primary(&self) -> bool {
        matches!(self, Self::Primary(_))
    }

    /// Return true when another process already owns the single-instance lock.
    pub fn is_duplicate(&self) -> bool {
        matches!(self, Self::Duplicate { .. })
    }

    /// Return the primary instance guard, if this process owns it.
    pub fn primary(&self) -> Option<&SingleInstance> {
        match self {
            Self::Primary(instance) => Some(instance),
            Self::Duplicate { .. } => None,
        }
    }

    /// Consume and return the primary instance guard, if this process owns it.
    pub fn into_primary(self) -> Option<SingleInstance> {
        match self {
            Self::Primary(instance) => Some(instance),
            Self::Duplicate { .. } => None,
        }
    }

    /// Return whether a duplicate launch notified the existing instance.
    pub fn notified_existing(&self) -> bool {
        matches!(self, Self::Duplicate { notified: true, .. })
    }

    /// Human-readable, deterministic summary for startup logs and agent audits.
    pub fn to_text(&self) -> String {
        match self {
            Self::Primary(instance) => {
                format!("single-instance primary for {}", instance.app_id())
            }
            Self::Duplicate { app_id, notified } => {
                let notification = if *notified {
                    "notified existing instance"
                } else {
                    "did not notify existing instance"
                };
                format!("single-instance duplicate for {app_id}: {notification}")
            }
        }
    }
}

/// Builder for native desktop single-instance startup handling.
#[derive(Debug, Clone)]
pub struct SingleInstanceBuilder {
    app_id: String,
    notify_existing: bool,
}

impl SingleInstanceBuilder {
    /// Create a builder for the given application identifier.
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            notify_existing: true,
        }
    }

    /// Set whether duplicate launches should notify the primary instance.
    pub fn notify_existing(mut self, notify_existing: bool) -> Self {
        self.notify_existing = notify_existing;
        self
    }

    /// Return the configured application identifier.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Return whether duplicate launches notify the primary instance.
    pub fn should_notify_existing(&self) -> bool {
        self.notify_existing
    }

    /// Validate the builder before attempting to acquire the lock.
    pub fn validate(&self) -> Result<()> {
        validate_single_instance_app_id(&self.app_id)
    }

    /// Acquire the single-instance lock or notify the already-running process.
    pub fn launch(self) -> Result<SingleInstanceLaunch> {
        self.validate()?;
        match SingleInstance::try_acquire(&self.app_id)? {
            Some(instance) => Ok(SingleInstanceLaunch::Primary(instance)),
            None => {
                let notified = if self.notify_existing {
                    send_activate_to_existing(&self.app_id)?;
                    true
                } else {
                    false
                };
                Ok(SingleInstanceLaunch::Duplicate {
                    app_id: self.app_id,
                    notified,
                })
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
impl SingleInstance {
    fn platform_try_acquire(app_id: &str) -> Result<Option<Self>> {
        use std::fs::OpenOptions;
        use std::os::fd::AsRawFd as _;
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        use std::os::unix::net::UnixListener;

        let path = socket_path(app_id)?;
        let lock_path = lock_path(&path);
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&lock_path)
            .map_err(|error| {
                anyhow::anyhow!("opening single-instance lock {lock_path:?}: {error}")
            })?;
        let lock_result =
            unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if lock_result != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(None);
            }
            return Err(anyhow::anyhow!(
                "locking single-instance file {lock_path:?}: {error}"
            ));
        }

        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "removing stale single-instance socket {path:?}: {error}"
                ));
            }
        }
        let listener = UnixListener::bind(&path)
            .map_err(|error| anyhow::anyhow!("binding single-instance socket {path:?}: {error}"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| anyhow::anyhow!("securing single-instance socket {path:?}: {error}"),
        )?;

        Ok(Some(Self {
            app_id: app_id.to_string(),
            _listener: listener,
            _socket_path: path,
            _lock_file: lock_file,
            activation_listener_started: AtomicBool::new(false),
            activation_stop: Arc::new(AtomicBool::new(false)),
        }))
    }

    fn platform_on_activate(&self, callback: Box<dyn Fn() + Send + 'static>) {
        use std::io::Read;
        use std::os::unix::net::UnixListener;
        use std::sync::atomic::Ordering;

        if self
            .activation_listener_started
            .swap(true, Ordering::AcqRel)
        {
            log::warn!("ignoring duplicate single-instance activation listener registration");
            return;
        }

        let listener = unsafe {
            use std::os::unix::io::{AsRawFd, FromRawFd};
            let fd = self._listener.as_raw_fd();
            let dup_fd = libc::dup(fd);
            if dup_fd < 0 {
                self.activation_listener_started
                    .store(false, Ordering::Release);
                log::error!(
                    "failed to duplicate single-instance activation socket: {}",
                    std::io::Error::last_os_error()
                );
                return;
            }
            UnixListener::from_raw_fd(dup_fd)
        };
        if let Err(error) = listener.set_nonblocking(true) {
            self.activation_listener_started
                .store(false, Ordering::Release);
            log::error!("failed to configure single-instance activation socket: {error}");
            return;
        }
        let stop = self.activation_stop.clone();
        let listener_thread = std::thread::Builder::new()
            .name(format!("single-instance-{}", self.app_id))
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _address)) => {
                            if let Err(error) = stream.set_nonblocking(false) {
                                log::warn!("failed to configure activation client socket: {error}");
                                continue;
                            }
                            if let Err(error) =
                                stream.set_read_timeout(Some(std::time::Duration::from_secs(1)))
                            {
                                log::warn!("failed to set activation socket read timeout: {error}");
                            }
                            let mut message = [0u8; 8];
                            if stream.read_exact(&mut message).is_ok() && &message == b"activate" {
                                crate::platform::catch_platform_callback(
                                    "single instance",
                                    "activation",
                                    (),
                                    &callback,
                                );
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(25));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => {
                            log::debug!("single-instance activation listener stopped: {error}");
                            break;
                        }
                    }
                }
            });
        if let Err(error) = listener_thread {
            self.activation_listener_started
                .store(false, Ordering::Release);
            log::error!("failed to spawn single-instance activation listener: {error}");
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
impl Drop for SingleInstance {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;

        self.activation_stop.store(true, Ordering::Release);
        let _ = std::fs::remove_file(&self._socket_path);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
fn platform_send_activate(app_id: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let path = socket_path(app_id)?;
    let mut stream = UnixStream::connect(&path)?;
    stream.write_all(b"activate")?;
    Ok(())
}

#[cfg(target_os = "windows")]
impl SingleInstance {
    fn platform_try_acquire(app_id: &str) -> Result<Option<Self>> {
        use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
        use windows::Win32::Foundation::GetLastError;
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::core::HSTRING;

        let name = HSTRING::from(format!("Global\\{}", app_id));
        unsafe {
            let handle = CreateMutexW(None, true, &name)?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return Ok(None);
            }
            Ok(Some(Self {
                app_id: app_id.to_string(),
                _mutex: WindowsMutexHandle { handle },
            }))
        }
    }

    fn platform_on_activate(&self, _callback: Box<dyn Fn() + Send + 'static>) {}
}

#[cfg(target_os = "windows")]
fn platform_send_activate(_app_id: &str) -> Result<()> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn platform_send_activate(_app_id: &str) -> Result<()> {
    anyhow::bail!("single-instance activation is not supported in browser builds")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_app_id(name: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        format!("kael-{name}-{}-{nanos}", std::process::id())
    }

    #[test]
    fn single_instance_builder_validates_app_id() {
        assert!(SingleInstanceBuilder::new("").validate().is_err());
        assert!(
            SingleInstanceBuilder::new(" com.example.app")
                .validate()
                .is_err()
        );
        assert!(
            SingleInstanceBuilder::new("com.example.app ")
                .validate()
                .is_err()
        );
        assert!(
            SingleInstanceBuilder::new("com.example/app")
                .validate()
                .is_err()
        );
        assert!(
            SingleInstanceBuilder::new("com.example\\app")
                .validate()
                .is_err()
        );
        assert!(
            SingleInstanceBuilder::new("com.example.\napp")
                .validate()
                .is_err()
        );
        assert!(
            SingleInstanceBuilder::new("a".repeat(129))
                .validate()
                .is_err()
        );
        assert!(
            SingleInstanceBuilder::new("com.example.app")
                .validate()
                .is_ok()
        );
        assert!(SingleInstance::try_acquire("/tmp/escaped").is_err());
        assert!(send_activate_to_existing("../escaped").is_err());
    }

    #[test]
    fn single_instance_builder_reports_primary_and_duplicate() {
        let app_id = unique_app_id("sib");
        let primary = SingleInstanceBuilder::new(&app_id).launch().unwrap();

        assert!(primary.is_primary());
        assert!(!primary.is_duplicate());
        assert_eq!(primary.app_id(), app_id);
        assert!(primary.primary().is_some());
        assert_eq!(primary.primary().unwrap().app_id(), app_id);
        assert_eq!(
            primary.to_text(),
            format!("single-instance primary for {app_id}")
        );

        let duplicate = SingleInstanceBuilder::new(&app_id)
            .notify_existing(false)
            .launch()
            .unwrap();

        assert!(duplicate.is_duplicate());
        assert!(!duplicate.is_primary());
        assert_eq!(duplicate.app_id(), app_id);
        assert!(!duplicate.notified_existing());
        assert_eq!(
            duplicate.to_text(),
            format!("single-instance duplicate for {app_id}: did not notify existing instance")
        );
        match duplicate {
            SingleInstanceLaunch::Duplicate {
                app_id: duplicate_app_id,
                notified,
            } => {
                assert_eq!(duplicate_app_id, app_id);
                assert!(!notified);
            }
            SingleInstanceLaunch::Primary(_) => panic!("expected duplicate launch"),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    #[test]
    fn activation_listener_contains_callback_panics_and_keeps_listening() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        };
        use std::time::Duration;

        let app_id = unique_app_id("activation");
        let instance = SingleInstance::try_acquire(&app_id)
            .expect("lock acquisition should not fail")
            .expect("test process should be primary");
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = calls.clone();
        let (tx, rx) = mpsc::channel();
        instance.on_activate(Box::new(move || {
            let call = callback_calls.fetch_add(1, Ordering::SeqCst);
            let _ = tx.send(call);
            if call == 0 {
                panic!("first activation callback panic should be contained");
            }
        }));

        send_activate_to_existing(&app_id).expect("first activation should be delivered");
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(0));

        send_activate_to_existing(&app_id).expect("second activation should be delivered");
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(1));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
