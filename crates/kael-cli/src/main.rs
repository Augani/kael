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

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq, Eq)]
pub enum ScaffoldError {
    InvalidName(String),
    AlreadyExists(PathBuf),
    Io(String),
}

impl fmt::Display for ScaffoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScaffoldError::InvalidName(name) => write!(
                f,
                "invalid project name ({} bytes): use at most 64 ASCII letters, digits, '-' or '_', not starting with a digit",
                name.len()
            ),
            ScaffoldError::AlreadyExists(path) => {
                write!(f, "destination {} already exists", path.display())
            }
            ScaffoldError::Io(message) => write!(f, "{message}"),
        }
    }
}

fn is_valid_project_name(name: &str) -> bool {
    if name.len() > 64 {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Scaffold a new Kael app named `name` under `dest`, returning the files written.
///
/// Creates `dest/<name>/` with `Cargo.toml`, `src/main.rs`, `README.md`, and
/// `.gitignore`. Fails without writing anything if the name is invalid or the
/// destination directory already exists.
pub fn scaffold(name: &str, dest: &Path) -> Result<Vec<PathBuf>, ScaffoldError> {
    if !is_valid_project_name(name) {
        return Err(ScaffoldError::InvalidName(name.to_string()));
    }

    let root = dest.join(name);
    match fs::create_dir(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ScaffoldError::AlreadyExists(root));
        }
        Err(error) => return Err(ScaffoldError::Io(error.to_string())),
    }
    let src = root.join("src");
    let result = (|| {
        fs::create_dir(&src).map_err(|err| ScaffoldError::Io(err.to_string()))?;

        let files = [
            (root.join("Cargo.toml"), CARGO_TEMPLATE),
            (src.join("main.rs"), MAIN_TEMPLATE),
            (root.join("README.md"), README_TEMPLATE),
            (root.join(".gitignore"), GITIGNORE_TEMPLATE),
        ];

        let mut written = Vec::with_capacity(files.len());
        for (path, template) in files {
            let contents = template.replace("{{name}}", name);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|err| ScaffoldError::Io(err.to_string()))?;
            use std::io::Write as _;
            file.write_all(contents.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|err| ScaffoldError::Io(err.to_string()))?;
            written.push(path);
        }
        sync_directory(&root).map_err(|err| ScaffoldError::Io(err.to_string()))?;
        Ok(written)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&root);
    }
    result
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
        "kael {VERSION} — tools for the Kael UI framework

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
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 3 {
        eprintln!("error: too many arguments");
        return ExitCode::FAILURE;
    }
    match args.get(1).map(String::as_str) {
        Some("new") => run_new(args.get(2).map(String::as_str)),
        Some("-V") | Some("--version") => {
            println!("kael {VERSION}");
            ExitCode::SUCCESS
        }
        Some("-h") | Some("--help") | None => {
            print_help();
            ExitCode::SUCCESS
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

        assert_eq!(written.len(), 4);
        for path in &written {
            assert!(path.exists(), "{} should exist", path.display());
        }

        let cargo = fs::read_to_string(dest.join("my_app/Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my_app\""));
        assert!(cargo.contains("kael = \"0.3\""));
        assert!(cargo.contains("kael_ui = \"0.3\""));

        let main = fs::read_to_string(dest.join("my_app/src/main.rs")).unwrap();
        assert!(main.contains("Hello, Kael!"));
        assert!(main.contains("Some(\"my_app\".into())"));
        assert!(main.contains("Application::try_new()?"));
        assert!(!main.contains(".unwrap()"));
        assert!(
            !main.contains("{{name}}"),
            "all placeholders must be substituted"
        );

        assert!(dest.join("my_app/README.md").exists());
        assert!(dest.join("my_app/.gitignore").exists());

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
        for bad in ["1app", "my app", "weird!", "", &"x".repeat(65)] {
            assert!(
                matches!(scaffold(bad, &dest), Err(ScaffoldError::InvalidName(_))),
                "{bad:?} must be rejected"
            );
        }
        assert!(is_valid_project_name("my_app"));
        assert!(is_valid_project_name("my-app"));
        assert!(is_valid_project_name("App2"));
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
