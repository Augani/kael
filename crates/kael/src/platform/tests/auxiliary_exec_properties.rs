// Feature: platform-parity-electron-features, Property 7: Auxiliary executable path resolution

use proptest::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Creates a unique temporary directory for test isolation.
/// Returns the path; caller is responsible for cleanup.
fn create_temp_dir(label: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "gpui_aux_exec_test_{label}_{}_{id}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

/// Resolves an auxiliary executable by searching the application directory first,
/// then falling back to additional search directories (simulating PATH lookup).
///
/// This replicates the core logic shared across platform implementations:
/// - macOS: searches the app bundle via `NSBundle.URLForAuxiliaryExecutable`
/// - Windows: searches `app_path().parent()` then `PATH`
/// - Linux: (to be implemented) same directory-first strategy
fn resolve_auxiliary_executable(
    name: &str,
    app_dir: &Path,
    extra_dirs: &[PathBuf],
) -> anyhow::Result<PathBuf> {
    // Search the application directory first
    let candidate = app_dir.join(name);
    if candidate.exists() {
        return Ok(candidate);
    }

    // On Windows the implementation also tries appending ".exe"
    #[cfg(target_os = "windows")]
    if !name.ends_with(".exe") {
        let candidate_exe = app_dir.join(format!("{name}.exe"));
        if candidate_exe.exists() {
            return Ok(candidate_exe);
        }
    }

    // Fall back to extra search directories (simulates PATH)
    for dir in extra_dirs {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
        #[cfg(target_os = "windows")]
        if !name.ends_with(".exe") {
            let candidate_exe = dir.join(format!("{name}.exe"));
            if candidate_exe.exists() {
                return Ok(candidate_exe);
            }
        }
    }

    anyhow::bail!("could not find auxiliary executable '{name}'")
}

/// Generates a valid binary file name.
///
/// Binary names are non-empty strings of alphanumeric characters and
/// hyphens/underscores, between 1 and 30 characters long. This covers
/// typical executable names like `my-tool`, `helper_bin`, `cli2`, etc.
fn binary_name_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9_-]{0,29}"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 12.4**
    ///
    /// For any binary name that exists in the application's installation
    /// directory, `path_for_auxiliary_executable` returns a `PathBuf` that
    /// points to an existing file on disk.
    #[test]
    fn auxiliary_executable_resolves_to_existing_file(name in binary_name_strategy()) {
        let app_dir = create_temp_dir("app");
        let binary_path = app_dir.join(&name);
        fs::write(&binary_path, b"fake-executable-content").unwrap();

        let result = resolve_auxiliary_executable(&name, &app_dir, &[]);
        let _ = fs::remove_dir_all(&app_dir);

        prop_assert!(result.is_ok(), "resolution must succeed for a binary that exists in the app directory");
        let resolved = result.unwrap();
        prop_assert_eq!(
            resolved, binary_path,
            "resolved path must point to the binary in the app directory"
        );
    }

    /// **Validates: Requirements 12.4**
    ///
    /// For any binary name that exists in a PATH directory (but not the app
    /// directory), `path_for_auxiliary_executable` still resolves to an
    /// existing file via the fallback search.
    #[test]
    fn auxiliary_executable_resolves_via_path_fallback(name in binary_name_strategy()) {
        let app_dir = create_temp_dir("app_empty");
        let path_dir = create_temp_dir("path");

        // Binary is NOT in app_dir, but IS in path_dir
        let binary_path = path_dir.join(&name);
        fs::write(&binary_path, b"fake-executable-content").unwrap();

        let result = resolve_auxiliary_executable(&name, &app_dir, &[path_dir.clone()]);
        let _ = fs::remove_dir_all(&app_dir);
        let _ = fs::remove_dir_all(&path_dir);

        prop_assert!(result.is_ok(), "resolution must succeed when binary exists in a PATH directory");
        prop_assert_eq!(
            result.unwrap(), binary_path,
            "resolved path must point to the binary in the PATH directory"
        );
    }

    /// **Validates: Requirements 12.4**
    ///
    /// For any binary name that does NOT exist in any search directory,
    /// `path_for_auxiliary_executable` returns an error rather than a
    /// non-existent path.
    #[test]
    fn auxiliary_executable_errors_for_missing_binary(name in binary_name_strategy()) {
        let app_dir = create_temp_dir("app_miss");

        let result = resolve_auxiliary_executable(&name, &app_dir, &[]);
        let _ = fs::remove_dir_all(&app_dir);

        prop_assert!(result.is_err(), "resolution must fail when the binary does not exist anywhere");
    }

    /// **Validates: Requirements 12.4**
    ///
    /// When the same binary name exists in both the app directory and a PATH
    /// directory, the app directory version takes precedence.
    #[test]
    fn auxiliary_executable_prefers_app_dir_over_path(name in binary_name_strategy()) {
        let app_dir = create_temp_dir("app_pref");
        let path_dir = create_temp_dir("path_pref");

        let app_binary = app_dir.join(&name);
        let path_binary = path_dir.join(&name);
        fs::write(&app_binary, b"app-version").unwrap();
        fs::write(&path_binary, b"path-version").unwrap();

        let result = resolve_auxiliary_executable(&name, &app_dir, &[path_dir.clone()]);
        let _ = fs::remove_dir_all(&app_dir);
        let _ = fs::remove_dir_all(&path_dir);

        prop_assert!(result.is_ok());
        let resolved = result.unwrap();
        prop_assert!(
            resolved.starts_with(&app_dir),
            "app directory must take precedence over PATH directories"
        );
    }
}
