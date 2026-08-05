//! `kael` command-line tool: scaffold new Kael applications.
//!
//! `kael new <name>` writes a minimal, ready-to-run app into `./<name>`, with
//! its templates embedded in the binary so it works offline once installed via
//! `cargo install kael-cli`.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const CARGO_TEMPLATE: &str = include_str!("../template/Cargo.toml.tmpl");
const MAIN_TEMPLATE: &str = include_str!("../template/main.rs.tmpl");
const README_TEMPLATE: &str = include_str!("../template/README.md.tmpl");
const GITIGNORE_TEMPLATE: &str = include_str!("../template/gitignore.tmpl");
const RUST_TOOLCHAIN_TEMPLATE: &str = include_str!("../template/rust-toolchain.toml.tmpl");

const VERSION: &str = env!("CARGO_PKG_VERSION");
const RUST_VERSION: &str = env!("CARGO_PKG_RUST_VERSION");

#[derive(Debug, PartialEq, Eq)]
pub enum ScaffoldError {
    InvalidName { name: String, reason: &'static str },
    AlreadyExists(PathBuf),
    Io(String),
}

impl fmt::Display for ScaffoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScaffoldError::InvalidName { name, reason } => {
                write!(f, "invalid project name {name:?}: {reason}")
            }
            ScaffoldError::AlreadyExists(path) => {
                write!(f, "destination {} already exists", path.display())
            }
            ScaffoldError::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ScaffoldError {}

fn validate_project_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 64 {
        return Err("use between 1 and 64 ASCII characters");
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return Err("start with an ASCII letter or underscore"),
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        return Err("use only ASCII letters, digits, hyphens, or underscores");
    }
    if is_rust_keyword(name) {
        return Err("Rust keywords cannot be Cargo package names");
    }
    if matches!(name, "test" | "build" | "deps" | "examples" | "incremental") {
        return Err("this name is reserved by Cargo");
    }
    let lowercase = name.to_ascii_lowercase();
    if matches!(lowercase.as_str(), "con" | "prn" | "aux" | "nul")
        || lowercase
            .strip_prefix("com")
            .is_some_and(is_windows_device_number)
        || lowercase
            .strip_prefix("lpt")
            .is_some_and(is_windows_device_number)
    {
        return Err("this name is reserved by Windows");
    }
    Ok(())
}

fn is_windows_device_number(suffix: &str) -> bool {
    matches!(suffix.as_bytes(), [b'1'..=b'9'])
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "_" | "as"
            | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "gen"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    )
}

/// Scaffold a new Kael app named `name` under `dest`, returning the files written.
///
/// Creates `dest/<name>/` with `Cargo.toml`, `src/main.rs`, `README.md`,
/// `.gitignore`, and `rust-toolchain.toml`. Invalid names and existing
/// destinations are rejected before writing; later I/O failures trigger a
/// best-effort rollback of the new tree.
pub fn scaffold(name: &str, dest: &Path) -> Result<Vec<PathBuf>, ScaffoldError> {
    validate_project_name(name).map_err(|reason| ScaffoldError::InvalidName {
        name: name.to_owned(),
        reason,
    })?;

    let root = dest.join(name);
    match fs::create_dir(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ScaffoldError::AlreadyExists(root));
        }
        Err(error) => return Err(io_error("failed to create", &root, error)),
    }
    let src = root.join("src");
    let result = (|| {
        fs::create_dir(&src).map_err(|error| io_error("failed to create", &src, error))?;

        let files = [
            (root.join("Cargo.toml"), CARGO_TEMPLATE),
            (src.join("main.rs"), MAIN_TEMPLATE),
            (root.join("README.md"), README_TEMPLATE),
            (root.join(".gitignore"), GITIGNORE_TEMPLATE),
            (root.join("rust-toolchain.toml"), RUST_TOOLCHAIN_TEMPLATE),
        ];

        let mut written = Vec::with_capacity(files.len());
        for (path, template) in files {
            let contents = template
                .replace("{{name}}", name)
                .replace("{{kael_version}}", VERSION)
                .replace("{{rust_version}}", RUST_VERSION);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|error| io_error("failed to create", &path, error))?;
            use std::io::Write as _;
            file.write_all(contents.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|error| io_error("failed to write", &path, error))?;
            written.push(path);
        }
        sync_directory(&src).map_err(|error| io_error("failed to sync", &src, error))?;
        sync_directory(&root).map_err(|error| io_error("failed to sync", &root, error))?;
        Ok(written)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&root);
    }
    result
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> ScaffoldError {
    ScaffoldError::Io(format!("{action} {}: {error}", path.display()))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn print_help() {
    println!(
        "kael {VERSION} — tools for the Kael native application framework

USAGE:
    kael <COMMAND>

COMMANDS:
    new <name>    Scaffold a new Kael app in ./<name>

OPTIONS:
    -h, --help       Print this help
    -V, --version    Print the version"
    );
}

fn run_new(name: Option<&str>) -> ExitCode {
    let Some(name) = name else {
        eprintln!("error: `kael new` requires a project name\n\n    kael new <name>");
        return ExitCode::FAILURE;
    };

    let dest = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("error: cannot resolve the current directory: {err}");
            return ExitCode::FAILURE;
        }
    };

    match scaffold(name, &dest) {
        Ok(_) => {
            println!(
                "Created Kael app `{name}` in ./{name}\n\nNext steps:\n    cd {name}\n    cargo run"
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args = match std::env::args_os()
        .map(|argument| argument.into_string())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(args) => args,
        Err(_) => {
            eprintln!("error: command-line arguments must be valid UTF-8");
            return ExitCode::FAILURE;
        }
    };
    if args.len() > 3 {
        eprintln!("error: too many arguments");
        return ExitCode::FAILURE;
    }
    match args.get(1).map(String::as_str) {
        Some("new") => run_new(args.get(2).map(String::as_str)),
        Some("-V") | Some("--version") if args.len() == 2 => {
            println!("kael {VERSION}");
            ExitCode::SUCCESS
        }
        Some("-h") | Some("--help") if args.len() == 2 => {
            print_help();
            ExitCode::SUCCESS
        }
        None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("-V" | "--version" | "-h" | "--help") => {
            eprintln!("error: too many arguments");
            ExitCode::FAILURE
        }
        Some(other) => {
            eprintln!("error: unknown command {other:?}\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "kael-cli-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scaffold_writes_expected_files_with_name_substituted() {
        let dest = temp_dir();
        let written = scaffold("my_app", &dest).unwrap();

        assert_eq!(written.len(), 5);
        for path in &written {
            assert!(path.exists(), "{} should exist", path.display());
            let contents = fs::read_to_string(path).unwrap();
            assert!(
                !contents.contains("{{"),
                "{} contains an unreplaced template marker",
                path.display()
            );
        }

        let cargo = fs::read_to_string(dest.join("my_app/Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my_app\""));
        assert!(cargo.contains(&format!("rust-version = \"{RUST_VERSION}\"")));
        assert!(cargo.contains(&format!(
            "kael = {{ version = \"{VERSION}\", features = [\"runtime_shaders\"] }}"
        )));
        assert!(cargo.contains(&format!("kael_ui = \"{VERSION}\"")));

        let main = fs::read_to_string(dest.join("my_app/src/main.rs")).unwrap();
        assert!(main.contains("Hello, Kael!"));
        assert!(main.contains("Some(\"my_app\".into())"));
        assert!(main.contains("Application::try_new()?"));
        assert!(!main.contains(".unwrap()"));
        assert!(dest.join("my_app/README.md").exists());
        assert!(dest.join("my_app/.gitignore").exists());
        let toolchain = fs::read_to_string(dest.join("my_app/rust-toolchain.toml")).unwrap();
        assert!(toolchain.contains(&format!("channel = \"{RUST_VERSION}\"")));

        fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn scaffold_refuses_to_overwrite_existing_directory() {
        let dest = temp_dir();
        scaffold("dupe", &dest).unwrap();
        let err = scaffold("dupe", &dest).unwrap_err();
        assert!(matches!(err, ScaffoldError::AlreadyExists(_)));
        fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn scaffold_rejects_invalid_names() {
        let dest = temp_dir();
        for bad in [
            "1app",
            "my app",
            "weird!",
            "",
            &"x".repeat(65),
            "type",
            "gen",
            "test",
            "build",
            "CON",
            "nul",
            "com1",
            "LPT9",
        ] {
            assert!(
                matches!(scaffold(bad, &dest), Err(ScaffoldError::InvalidName { .. })),
                "{bad:?} must be rejected"
            );
        }
        assert!(validate_project_name("my_app").is_ok());
        assert!(validate_project_name("my-app").is_ok());
        assert!(validate_project_name("App2").is_ok());
        assert!(validate_project_name("com0").is_ok());
        fs::remove_dir_all(&dest).ok();
    }

    #[cfg(unix)]
    #[test]
    fn scaffold_treats_dangling_symlink_as_existing_destination() {
        let dest = temp_dir();
        let root = dest.join("linked");
        std::os::unix::fs::symlink(dest.join("missing-target"), &root).unwrap();
        assert!(matches!(
            scaffold("linked", &dest),
            Err(ScaffoldError::AlreadyExists(path)) if path == root
        ));
        assert!(
            fs::symlink_metadata(&root)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_file(root).ok();
        fs::remove_dir_all(dest).ok();
    }

    #[test]
    fn concurrent_scaffolds_never_overwrite_each_other() {
        let dest = temp_dir();
        let first_dest = dest.clone();
        let second_dest = dest.clone();
        let first = std::thread::spawn(move || scaffold("race", &first_dest));
        let second = std::thread::spawn(move || scaffold("race", &second_dest));
        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(ScaffoldError::AlreadyExists(_))))
                .count(),
            1
        );
        fs::remove_dir_all(dest).ok();
    }
}
