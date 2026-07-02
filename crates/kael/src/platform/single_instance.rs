/// Cross-platform single instance enforcement for applications.
///
/// Uses Unix domain sockets on macOS/Linux and named mutexes on Windows
/// to ensure only one instance of an application runs at a time.
use anyhow::Result;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
use std::path::PathBuf;

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
fn socket_path(app_id: &str) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(dir).join(format!("{}.sock", app_id))
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
        Self::platform_acquire(app_id)
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

/// Send an activation message to an already-running instance of the application.
///
/// This is typically called after `SingleInstance::acquire` returns `Err(AlreadyRunning)`
/// to signal the existing instance to come to the foreground.
pub fn send_activate_to_existing(app_id: &str) -> Result<()> {
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
}

/// Builder for Electron-style single-instance startup handling.
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
        anyhow::ensure!(
            !self.app_id.trim().is_empty(),
            "single-instance app id cannot be empty"
        );
        anyhow::ensure!(
            self.app_id == self.app_id.trim(),
            "single-instance app id cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            self.app_id.len() <= 128,
            "single-instance app id cannot be longer than 128 bytes"
        );
        anyhow::ensure!(
            !self.app_id.chars().any(char::is_control),
            "single-instance app id cannot contain control characters"
        );
        anyhow::ensure!(
            !self.app_id.contains('/') && !self.app_id.contains('\\'),
            "single-instance app id cannot contain path separators"
        );
        Ok(())
    }

    /// Acquire the single-instance lock or notify the already-running process.
    pub fn launch(self) -> Result<SingleInstanceLaunch> {
        self.validate()?;
        match SingleInstance::acquire(&self.app_id) {
            Ok(instance) => Ok(SingleInstanceLaunch::Primary(instance)),
            Err(_) => {
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
    fn platform_acquire(app_id: &str) -> std::result::Result<Self, AlreadyRunning> {
        use std::os::unix::net::UnixListener;

        let path = socket_path(app_id);

        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Err(AlreadyRunning);
        }

        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).map_err(|_| AlreadyRunning)?;
        listener.set_nonblocking(true).ok();

        Ok(Self {
            app_id: app_id.to_string(),
            _listener: listener,
            _socket_path: path,
        })
    }

    fn platform_on_activate(&self, callback: Box<dyn Fn() + Send + 'static>) {
        use std::io::Read;
        use std::os::unix::net::UnixListener;

        let listener = unsafe {
            use std::os::unix::io::{AsRawFd, FromRawFd};
            let fd = self._listener.as_raw_fd();
            let dup_fd = libc::dup(fd);
            if dup_fd < 0 {
                return;
            }
            UnixListener::from_raw_fd(dup_fd)
        };
        listener.set_nonblocking(false).ok();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut buf = [0u8; 64];
                        if let Ok(n) = stream.read(&mut buf) {
                            if n > 0 && &buf[..n.min(8)] == b"activate" {
                                callback();
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self._socket_path);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
fn platform_send_activate(app_id: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let path = socket_path(app_id);
    let mut stream = UnixStream::connect(&path)?;
    stream.write_all(b"activate")?;
    Ok(())
}

#[cfg(target_os = "windows")]
impl SingleInstance {
    fn platform_acquire(app_id: &str) -> std::result::Result<Self, AlreadyRunning> {
        use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
        use windows::Win32::Foundation::GetLastError;
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::core::HSTRING;

        let name = HSTRING::from(format!("Global\\{}", app_id));
        unsafe {
            let handle = CreateMutexW(None, true, &name).map_err(|_| AlreadyRunning)?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return Err(AlreadyRunning);
            }
            Ok(Self {
                app_id: app_id.to_string(),
                _mutex: WindowsMutexHandle { handle },
            })
        }
    }

    fn platform_on_activate(&self, _callback: Box<dyn Fn() + Send + 'static>) {}
}

#[cfg(target_os = "windows")]
fn platform_send_activate(_app_id: &str) -> Result<()> {
    Ok(())
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

        let duplicate = SingleInstanceBuilder::new(&app_id)
            .notify_existing(false)
            .launch()
            .unwrap();

        assert!(duplicate.is_duplicate());
        assert!(!duplicate.is_primary());
        assert_eq!(duplicate.app_id(), app_id);
        assert!(!duplicate.notified_existing());
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
}
