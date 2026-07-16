//! Platform-agnostic apply and rollback machinery for installing updates.
//!
//! The pure logic — version comparison, staging path computation, and the
//! rollback state machine — lives here and is fully unit-tested. Filesystem
//! side effects are abstracted behind [`InstallerBackend`] so the swap/rollback
//! orchestration can be exercised with a mock backend; [`FsInstaller`] is the
//! real implementation that renames directories on disk.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use sha2::{Digest as _, Sha256};

/// Compares two dotted `major.minor.patch` versions, returning whether
/// `candidate` is strictly newer than `current`.
///
/// Returns `false` if either version cannot be parsed, so an unparseable feed
/// version is never treated as an upgrade (fail closed).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_triplet(candidate), parse_triplet(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

fn parse_triplet(version: &str) -> Option<(u64, u64, u64)> {
    fn component(part: &str) -> Option<u64> {
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        part.parse().ok()
    }
    let mut parts = version.split('.');
    let major = component(parts.next()?)?;
    let minor = component(parts.next()?)?;
    let patch = component(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Computes a unique staging directory under `base` for a given update token.
///
/// The token is sanitized to a single, safe path component so a hostile feed
/// version or URL cannot escape `base` via traversal or separators.
pub fn staging_dir(base: &Path, token: &str) -> PathBuf {
    base.join(format!("kael_update_{}", safe_component(token)))
}

/// Computes the staging path for a downloaded artifact file inside `dir`.
pub fn staging_path(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(safe_component(file_name))
}

fn safe_component(raw: &str) -> String {
    let candidate = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let mut cleaned: String = candidate
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(80)
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        cleaned = "staged".to_string();
    }
    let digest = Sha256::digest(raw.as_bytes());
    format!("{cleaned}-{}", hex::encode(&digest[..8]))
}

/// The state of an atomic swap, advanced by [`atomic_swap_with_rollback`].
///
/// The transitions form a strict state machine:
/// `Idle -> BackedUp -> Swapped -> Committed`, with `RolledBack` reachable from
/// `BackedUp` or `Swapped` when a later step fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackState {
    /// Nothing has happened yet.
    Idle,
    /// The existing install was moved aside to a backup location.
    BackedUp,
    /// The new install was moved into place over the original location.
    Swapped,
    /// The backup was discarded; the swap is permanent.
    Committed,
    /// A failure occurred and the original install was restored.
    RolledBack,
}

impl RollbackState {
    /// Advances from `Idle` to `BackedUp`. Errors if called out of order.
    pub fn backed_up(self) -> Result<Self> {
        match self {
            Self::Idle => Ok(Self::BackedUp),
            other => bail!("invalid rollback transition: {other:?} -> BackedUp"),
        }
    }

    /// Advances from `BackedUp` to `Swapped`. Errors if called out of order.
    pub fn swapped(self) -> Result<Self> {
        match self {
            Self::BackedUp => Ok(Self::Swapped),
            other => bail!("invalid rollback transition: {other:?} -> Swapped"),
        }
    }

    /// Advances from `Swapped` to `Committed`. Errors if called out of order.
    pub fn committed(self) -> Result<Self> {
        match self {
            Self::Swapped => Ok(Self::Committed),
            other => bail!("invalid rollback transition: {other:?} -> Committed"),
        }
    }

    /// Marks the swap as rolled back. Valid from `BackedUp` or `Swapped`.
    pub fn rolled_back(self) -> Result<Self> {
        match self {
            Self::BackedUp | Self::Swapped => Ok(Self::RolledBack),
            other => bail!("invalid rollback transition: {other:?} -> RolledBack"),
        }
    }

    /// Whether the swap completed successfully.
    pub fn is_committed(&self) -> bool {
        matches!(self, Self::Committed)
    }
}

/// Filesystem operations required to perform an atomic install swap.
///
/// Real installs use [`FsInstaller`]; tests use a mock so the rollback state
/// machine can be driven through failure paths without touching disk.
pub trait InstallerBackend {
    /// Move `from` aside to `to` (the backup).
    fn backup(&self, from: &Path, to: &Path) -> Result<()>;
    /// Move the staged install `from` into the live location `to`.
    fn swap_in(&self, from: &Path, to: &Path) -> Result<()>;
    /// Restore the backup `from` back to the live location `to` after a failure.
    fn restore(&self, from: &Path, to: &Path) -> Result<()>;
    /// Discard the backup at `path` once the swap is committed.
    fn discard_backup(&self, path: &Path) -> Result<()>;
}

/// Paths involved in an atomic swap.
#[derive(Debug, Clone)]
pub struct SwapPlan {
    /// The currently-installed location to be replaced.
    pub live: PathBuf,
    /// The staged replacement to move into `live`.
    pub staged: PathBuf,
    /// Where the original `live` is moved before the swap.
    pub backup: PathBuf,
}

/// Performs an atomic swap with rollback using the given backend.
///
/// Steps: back up the live install, move the staged install into place, then
/// discard the backup. If moving the staged install in fails, the original is
/// restored from the backup. Returns the terminal [`RollbackState`].
pub fn atomic_swap_with_rollback(
    backend: &dyn InstallerBackend,
    plan: &SwapPlan,
) -> Result<RollbackState> {
    validate_swap_plan(plan)?;
    let mut state = RollbackState::Idle;

    backend.backup(&plan.live, &plan.backup)?;
    state = state.backed_up()?;

    if let Err(swap_err) = backend.swap_in(&plan.staged, &plan.live) {
        if let Err(restore_err) = backend.restore(&plan.backup, &plan.live) {
            bail!(
                "swap failed ({swap_err}) and rollback also failed ({restore_err}); \
                 install may be in an inconsistent state at {}",
                plan.live.display()
            );
        }
        debug_assert_eq!(state.rolled_back()?, RollbackState::RolledBack);
        return Err(swap_err);
    }
    state = state.swapped()?;

    if let Err(cleanup_err) = backend.discard_backup(&plan.backup) {
        bail!(
            "update was installed at {} but backup cleanup failed ({cleanup_err}); do not retry the swap automatically",
            plan.live.display()
        );
    }
    state = state.committed()?;

    Ok(state)
}

fn validate_swap_plan(plan: &SwapPlan) -> Result<()> {
    if plan.live == plan.staged || plan.live == plan.backup || plan.staged == plan.backup {
        bail!("live, staged, and backup paths must be distinct");
    }
    if plan.live.parent() != plan.backup.parent() {
        bail!("live and backup paths must share a parent for an atomic rename");
    }
    Ok(())
}

/// The real, on-disk [`InstallerBackend`] using atomic `rename` where possible.
#[derive(Debug, Default, Clone, Copy)]
pub struct FsInstaller;

impl InstallerBackend for FsInstaller {
    fn backup(&self, from: &Path, to: &Path) -> Result<()> {
        if to.exists() {
            bail!(
                "refusing to overwrite existing update backup: {}",
                to.display()
            );
        }
        std::fs::rename(from, to).map_err(|e| {
            anyhow::anyhow!(
                "failed to move {} to backup {}: {e}",
                from.display(),
                to.display()
            )
        })
    }

    fn swap_in(&self, from: &Path, to: &Path) -> Result<()> {
        std::fs::rename(from, to).map_err(|e| {
            anyhow::anyhow!(
                "failed to move staged install {} into {}: {e}",
                from.display(),
                to.display()
            )
        })
    }

    fn restore(&self, from: &Path, to: &Path) -> Result<()> {
        if !from.exists() {
            bail!("cannot restore missing update backup: {}", from.display());
        }
        if to.exists() {
            remove_path(to)?;
        }
        std::fs::rename(from, to).map_err(|e| {
            anyhow::anyhow!(
                "failed to restore backup {} to {}: {e}",
                from.display(),
                to.display()
            )
        })
    }

    fn discard_backup(&self, path: &Path) -> Result<()> {
        if path.exists() {
            remove_path(path)?;
        }
        Ok(())
    }
}

fn remove_path(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| anyhow::anyhow!("failed to stat {}: {e}", path.display()))?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
    .map_err(|e| anyhow::anyhow!("failed to remove {}: {e}", path.display()))
}

/// Verifies the code signature of a macOS bundle via `codesign --verify`.
///
/// Returns an error if `codesign` reports the bundle as invalid or if called on
/// a platform that cannot perform macOS code-signature verification.
pub fn verify_codesign(_bundle: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        if !_bundle.is_dir() {
            bail!("bundle is not a directory: {}", _bundle.display());
        }
        let status = std::process::Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(_bundle)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| anyhow::anyhow!("failed to run codesign: {e}"))?;
        if !status.success() {
            bail!("codesign verification failed for {}", _bundle.display());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        bail!("macOS code-signature verification is unavailable on this platform")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn is_newer_compares_triplets() {
        assert!(is_newer("1.2.4", "1.2.3"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("1.3.0", "1.2.9"));
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(!is_newer("1.2.3", "1.2.4"));
        assert!(!is_newer("0.9.0", "1.0.0"));
    }

    #[test]
    fn is_newer_fails_closed_on_unparseable() {
        assert!(!is_newer("not-a-version", "1.0.0"));
        assert!(!is_newer("1.0.0", "garbage"));
        assert!(!is_newer("1.2", "1.0.0"));
        assert!(!is_newer("1.2.3.4", "1.0.0"));
    }

    #[test]
    fn staging_paths_are_single_components() {
        use std::path::Component;
        let base = Path::new("/tmp/base");
        for token in [
            "1.2.3",
            "../../etc/passwd",
            "a/b/c",
            "weird?name#frag",
            "",
            "..",
        ] {
            let dir = staging_dir(base, token);
            let last = dir.file_name().unwrap().to_string_lossy().to_string();
            assert!(last.starts_with("kael_update_"));
            assert!(!last.contains('/') && !last.contains('\\'));

            let file = staging_path(&dir, token);
            let name = file.file_name().unwrap();
            let comps: Vec<_> = Path::new(name).components().collect();
            assert_eq!(comps.len(), 1);
            assert!(matches!(comps[0], Component::Normal(_)));
        }
    }

    #[test]
    fn staging_paths_do_not_collide_after_sanitization() {
        let base = Path::new("/tmp/base");
        assert_ne!(staging_dir(base, "a/b"), staging_dir(base, "b"));
        assert_ne!(staging_path(base, "a?x"), staging_path(base, "a#x"));
    }

    #[test]
    fn rejects_aliasing_swap_paths() {
        let backend = MockBackend::default();
        let mut alias = plan();
        alias.backup = alias.live.clone();
        assert!(atomic_swap_with_rollback(&backend, &alias).is_err());
        assert!(backend.calls.borrow().is_empty());
    }

    #[test]
    fn rollback_state_machine_rejects_out_of_order() {
        assert!(RollbackState::Idle.swapped().is_err());
        assert!(RollbackState::Idle.committed().is_err());
        assert!(RollbackState::Idle.rolled_back().is_err());
        assert!(RollbackState::Committed.rolled_back().is_err());

        let s = RollbackState::Idle.backed_up().unwrap();
        let s = s.swapped().unwrap();
        let s = s.committed().unwrap();
        assert!(s.is_committed());
    }

    #[derive(Default)]
    struct MockBackend {
        fail_swap: bool,
        fail_restore: bool,
        fail_discard: bool,
        calls: RefCell<Vec<&'static str>>,
    }

    impl InstallerBackend for MockBackend {
        fn backup(&self, _from: &Path, _to: &Path) -> Result<()> {
            self.calls.borrow_mut().push("backup");
            Ok(())
        }
        fn swap_in(&self, _from: &Path, _to: &Path) -> Result<()> {
            self.calls.borrow_mut().push("swap_in");
            if self.fail_swap {
                bail!("swap_in failed");
            }
            Ok(())
        }
        fn restore(&self, _from: &Path, _to: &Path) -> Result<()> {
            self.calls.borrow_mut().push("restore");
            if self.fail_restore {
                bail!("restore failed");
            }
            Ok(())
        }
        fn discard_backup(&self, _path: &Path) -> Result<()> {
            self.calls.borrow_mut().push("discard_backup");
            if self.fail_discard {
                bail!("discard failed");
            }
            Ok(())
        }
    }

    fn plan() -> SwapPlan {
        SwapPlan {
            live: PathBuf::from("/app/Kael.app"),
            staged: PathBuf::from("/staging/Kael.app"),
            backup: PathBuf::from("/app/Kael.app.backup"),
        }
    }

    #[test]
    fn atomic_swap_commits_on_success() {
        let backend = MockBackend::default();
        let state = atomic_swap_with_rollback(&backend, &plan()).unwrap();
        assert_eq!(state, RollbackState::Committed);
        assert_eq!(
            *backend.calls.borrow(),
            vec!["backup", "swap_in", "discard_backup"]
        );
    }

    #[test]
    fn atomic_swap_rolls_back_on_failed_swap() {
        let backend = MockBackend {
            fail_swap: true,
            ..Default::default()
        };
        let err = atomic_swap_with_rollback(&backend, &plan()).unwrap_err();
        assert!(err.to_string().contains("swap_in failed"));
        assert_eq!(
            *backend.calls.borrow(),
            vec!["backup", "swap_in", "restore"]
        );
    }

    #[test]
    fn atomic_swap_reports_inconsistent_state_when_rollback_also_fails() {
        let backend = MockBackend {
            fail_swap: true,
            fail_restore: true,
            ..Default::default()
        };
        let err = atomic_swap_with_rollback(&backend, &plan()).unwrap_err();
        assert!(err.to_string().contains("inconsistent state"));
    }

    #[test]
    fn atomic_swap_distinguishes_cleanup_failure_from_swap_failure() {
        let backend = MockBackend {
            fail_discard: true,
            ..Default::default()
        };
        let error = atomic_swap_with_rollback(&backend, &plan()).unwrap_err();
        assert!(error.to_string().contains("update was installed"));
        assert!(error.to_string().contains("do not retry"));
    }

    #[test]
    fn fs_installer_swaps_a_real_directory_with_rollback() {
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join("Kael.app");
        let staged = tmp.path().join("staging").join("Kael.app");
        let backup = tmp.path().join("Kael.app.backup");

        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("marker"), b"old").unwrap();
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("marker"), b"new").unwrap();

        let backend = FsInstaller;
        let swap_plan = SwapPlan {
            live: live.clone(),
            staged: staged.clone(),
            backup: backup.clone(),
        };
        let state = atomic_swap_with_rollback(&backend, &swap_plan).unwrap();
        assert!(state.is_committed());

        assert_eq!(std::fs::read(live.join("marker")).unwrap(), b"new");
        assert!(!backup.exists(), "backup must be discarded after commit");
        assert!(!staged.exists(), "staged dir was moved into place");
    }

    #[test]
    fn fs_installer_restores_when_swap_target_is_a_file_conflict() {
        // Force swap_in to fail by making the staged path nonexistent.
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join("Kael.app");
        let staged = tmp.path().join("does-not-exist.app");
        let backup = tmp.path().join("Kael.app.backup");

        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("marker"), b"old").unwrap();

        let backend = FsInstaller;
        let swap_plan = SwapPlan {
            live: live.clone(),
            staged,
            backup,
        };
        let result = atomic_swap_with_rollback(&backend, &swap_plan);
        assert!(result.is_err());
        // Original content must be restored after the failed swap.
        assert_eq!(std::fs::read(live.join("marker")).unwrap(), b"old");
    }

    #[test]
    fn fs_installer_preserves_preexisting_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join("Kael.app");
        let backup = tmp.path().join("Kael.app.backup");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("marker"), b"recovery").unwrap();

        assert!(FsInstaller.backup(&live, &backup).is_err());
        assert_eq!(std::fs::read(backup.join("marker")).unwrap(), b"recovery");
        assert!(live.exists());
    }
}
