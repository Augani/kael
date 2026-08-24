use anyhow::{Context as _, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Crates.io version requirement written into scaffolded projects.
const KAEL_VERSION: &str = "0.4";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Dashboard,
    Messaging,
    Workspace,
}

impl Template {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "dashboard" => Ok(Template::Dashboard),
            "messaging" => Ok(Template::Messaging),
            "workspace" => Ok(Template::Workspace),
            other => bail!("unknown template '{other}' (expected dashboard|messaging|workspace)"),
        }
    }

    /// The window title the source template hardcodes in `main.rs`.
    fn source_window_title(self) -> &'static str {
        match self {
            Template::Dashboard => "Acme Analytics",
            Template::Messaging => "Pulse — Messaging",
            Template::Workspace => "Kael Workspace",
        }
    }
}

pub struct ScaffoldOptions {
    pub name: String,
    pub template: Template,
    pub target_dir: PathBuf,
    pub app_id: Option<String>,
    pub local_dev: bool,
}

pub struct ScaffoldOutcome {
    pub target_dir: PathBuf,
    pub crate_name: String,
    pub app_name: String,
    pub app_id: String,
}

/// The `src/main.rs` for each template, embedded at compile time so the scaffolder is
/// self-contained and does not depend on the repository's `templates/` directory on disk
/// (lets it work as an installed `kael new` binary).
fn template_main_src(template: Template) -> &'static str {
    match template {
        Template::Dashboard => include_str!("../../templates/dashboard/src/main.rs"),
        Template::Messaging => include_str!("../../templates/messaging/src/main.rs"),
        Template::Workspace => include_str!("../../templates/workspace/src/main.rs"),
    }
}

pub fn run(options: &ScaffoldOptions) -> Result<ScaffoldOutcome> {
    let crate_name = sanitize_crate_name(&options.name)?;
    let app_name = display_name(&options.name);
    let app_id = options
        .app_id
        .clone()
        .unwrap_or_else(|| default_app_id(&crate_name));

    let target_existed = match fs::symlink_metadata(&options.target_dir) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!(
                    "target path already exists and is not a directory: {}",
                    options.target_dir.display()
                );
            }
            let is_empty = fs::read_dir(&options.target_dir)
                .with_context(|| format!("reading {}", options.target_dir.display()))?
                .next()
                .is_none();
            if !is_empty {
                bail!(
                    "target directory already exists and is not empty: {}",
                    options.target_dir.display()
                );
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspecting {}", options.target_dir.display()));
        }
    };

    let main_src = template_main_src(options.template);

    let cargo_toml = render_cargo_toml(options.template, &crate_name, options.local_dev);
    let dist_toml = render_dist_toml(&app_name, &app_id);
    let readme = render_readme(&app_name, &crate_name);
    let main_rs = main_src.replace(options.template.source_window_title(), &app_name);

    if !target_existed {
        fs::create_dir(&options.target_dir)
            .with_context(|| format!("creating {}", options.target_dir.display()))?;
    }
    let src_dir = options.target_dir.join("src");
    let files = [
        (options.target_dir.join("Cargo.toml"), cargo_toml.as_str()),
        (src_dir.join("main.rs"), main_rs.as_str()),
        (
            options.target_dir.join("kael.dist.toml"),
            dist_toml.as_str(),
        ),
        (options.target_dir.join("README.md"), readme.as_str()),
    ];

    let mut created_src = false;
    let mut created_files = 0;
    let result = (|| -> Result<()> {
        fs::create_dir(&src_dir).with_context(|| format!("creating {}", src_dir.display()))?;
        created_src = true;
        for (path, contents) in &files {
            write_new_file(path, contents)?;
            created_files += 1;
        }
        #[cfg(unix)]
        {
            fs::File::open(&src_dir)
                .and_then(|directory| directory.sync_all())
                .with_context(|| format!("syncing {}", src_dir.display()))?;
            fs::File::open(&options.target_dir)
                .and_then(|directory| directory.sync_all())
                .with_context(|| format!("syncing {}", options.target_dir.display()))?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        for (path, _) in files[..created_files].iter().rev() {
            let _ = fs::remove_file(path);
        }
        if created_src {
            let _ = fs::remove_dir(&src_dir);
        }
        if !target_existed {
            let _ = fs::remove_dir(&options.target_dir);
        }
        return Err(error);
    }

    Ok(ScaffoldOutcome {
        target_dir: options.target_dir.clone(),
        crate_name,
        app_name,
        app_id,
    })
}

fn write_new_file(path: &Path, contents: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o644);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("syncing {}", path.display()))
}

fn sanitize_crate_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("project name must not be empty");
    }
    if trimmed.chars().count() > 64 {
        bail!("project name must be at most 64 characters");
    }
    let sanitized: String = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches('-').to_string();
    if sanitized.is_empty() {
        bail!("project name '{name}' has no usable alphanumeric characters");
    }
    if sanitized.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        bail!("project name '{name}' must not start with a digit");
    }
    Ok(sanitized)
}

fn display_name(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() {
        name.to_string()
    } else {
        words.join(" ")
    }
}

fn default_app_id(crate_name: &str) -> String {
    format!("com.example.{}", crate_name.replace('-', ""))
}

fn render_cargo_toml(template: Template, crate_name: &str, local_dev: bool) -> String {
    let kael_ui_features = matches!(template, Template::Workspace);
    let (kael_dep, kael_ui_dep) = if local_dev {
        let kael = format!(
            "kael = {{ path = {:?} }}",
            local_crate_path("kael").display().to_string()
        );
        let ui_path = local_crate_path("kael_ui").display().to_string();
        let kael_ui = if kael_ui_features {
            format!("kael_ui = {{ path = {ui_path:?}, features = [\"editor-languages\"] }}")
        } else {
            format!("kael_ui = {{ path = {ui_path:?} }}")
        };
        (kael, kael_ui)
    } else {
        let kael = format!("kael = \"{KAEL_VERSION}\"");
        let kael_ui = if kael_ui_features {
            format!(
                "kael_ui = {{ version = \"{KAEL_VERSION}\", features = [\"editor-languages\"] }}"
            )
        } else {
            format!("kael_ui = \"{KAEL_VERSION}\"")
        };
        (kael, kael_ui)
    };

    format!(
        "[package]\n\
         name = {crate_name:?}\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         publish = false\n\
         \n\
         [dependencies]\n\
         {kael_dep}\n\
         {kael_ui_dep}\n"
    )
}

/// Absolute path to a workspace crate, used by `--local-dev` so a scaffolded
/// project can build and be tested against the in-tree crates without crates.io.
fn local_crate_path(crate_name: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.clone())
        .join("crates")
        .join(crate_name)
}

fn render_dist_toml(app_name: &str, app_id: &str) -> String {
    format!(
        "app_id = {app_id:?}\n\
         name = {app_name:?}\n\
         version = \"0.1.0\"\n\
         \n\
         [icons]\n\
         \n\
         [bundle]\n\
         copyright = \"© 2026 {app_name}\"\n\
         category = \"public.app-category.productivity\"\n\
         minimum_system_version = \"12.0\"\n\
         file_description = {app_name:?}\n\
         linux_categories = [\"Utility\"]\n\
         \n\
         [signing]\n\
         \n\
         [updater]\n\
         feed_url = \"https://example.com/updates/feed.json\"\n\
         artifact_base_url = \"https://example.com/downloads\"\n\
         # A real publish requires `public_key` plus the matching\n\
         # `KAEL_UPDATE_SIGNING_KEY`; generate them with `xtask generate-update-key`.\n"
    )
}

fn render_readme(app_name: &str, crate_name: &str) -> String {
    format!(
        "# {app_name}\n\
         \n\
         A Kael desktop application scaffolded from a template.\n\
         \n\
         ## Running\n\
         \n\
         ```sh\n\
         cargo run\n\
         ```\n\
         \n\
         ### Building without a Metal toolchain (macOS)\n\
         \n\
         A debug build compiles shaders ahead of time with Xcode's `metal` tool.\n\
         If you do not have the full Xcode command-line tools, enable the\n\
         `runtime_shaders` feature so shaders are compiled at launch instead:\n\
         \n\
         ```sh\n\
         cargo run --features kael/runtime_shaders\n\
         ```\n\
         \n\
         ## Packaging\n\
         \n\
         Edit `kael.dist.toml` (set a real `app_id`, signing identity, updater\n\
         feed URL, and artifact base URL), then build installers with the `xtask`\n\
         toolchain from the Kael\n\
         repository. A real updater publication also requires the public/private\n\
         key pair generated by `xtask generate-update-key`; select the canonical\n\
         updater installer with `xtask publish --update-artifact <path>`.\n\
         \n\
         ## Crate\n\
         \n\
         This project's crate is named `{crate_name}`.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sanitize_crate_name_lowercases_and_replaces() {
        assert_eq!(sanitize_crate_name("My App").unwrap(), "my-app");
        assert_eq!(sanitize_crate_name("Cool_Thing").unwrap(), "cool_thing");
        assert!(sanitize_crate_name("   ").is_err());
        assert!(sanitize_crate_name("9lives").is_err());
    }

    #[test]
    fn display_name_titlecases() {
        assert_eq!(display_name("my-cool-app"), "My Cool App");
        assert_eq!(display_name("pulse"), "Pulse");
    }

    #[test]
    fn default_app_id_strips_dashes() {
        assert_eq!(default_app_id("my-app"), "com.example.myapp");
    }

    #[test]
    fn render_cargo_toml_uses_version_deps_by_default() {
        let toml = render_cargo_toml(Template::Dashboard, "demo", false);
        assert!(toml.contains("name = \"demo\""));
        assert!(toml.contains(&format!("kael = \"{KAEL_VERSION}\"")));
        assert!(toml.contains(&format!("kael_ui = \"{KAEL_VERSION}\"")));
        assert!(!toml.contains("path ="));
    }

    #[test]
    fn render_cargo_toml_preserves_workspace_features() {
        let toml = render_cargo_toml(Template::Workspace, "ide", false);
        assert!(toml.contains("editor-languages"));
    }

    #[test]
    fn render_cargo_toml_local_dev_uses_path_deps() {
        let toml = render_cargo_toml(Template::Dashboard, "demo", true);
        assert!(toml.contains("path ="));
        assert!(!toml.contains("kael = \"0.4\""));
    }

    #[test]
    fn scaffold_writes_expected_structure_and_substitutions() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("my-dash");
        let options = ScaffoldOptions {
            name: "My Dash".to_string(),
            template: Template::Dashboard,
            target_dir: target.clone(),
            app_id: Some("com.acme.mydash".to_string()),
            local_dev: false,
        };

        let outcome = run(&options).unwrap();
        assert_eq!(outcome.crate_name, "my-dash");
        assert_eq!(outcome.app_name, "My Dash");
        assert_eq!(outcome.app_id, "com.acme.mydash");

        assert!(target.join("Cargo.toml").is_file());
        assert!(target.join("src/main.rs").is_file());
        assert!(target.join("kael.dist.toml").is_file());
        assert!(target.join("README.md").is_file());

        let cargo = fs::read_to_string(target.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my-dash\""));
        assert!(!cargo.contains("dashboard-app"));
        assert!(cargo.contains("kael = \"0.4\""));

        let dist = fs::read_to_string(target.join("kael.dist.toml")).unwrap();
        assert!(dist.contains("app_id = \"com.acme.mydash\""));
        assert!(dist.contains("name = \"My Dash\""));
        assert!(dist.contains("artifact_base_url = \"https://example.com/downloads\""));

        let main_rs = fs::read_to_string(target.join("src/main.rs")).unwrap();
        assert!(main_rs.contains("My Dash"));
        assert!(!main_rs.contains("Acme Analytics"));

        let readme = fs::read_to_string(target.join("README.md")).unwrap();
        assert!(readme.contains("runtime_shaders"));
        assert!(readme.contains("My Dash"));
    }

    #[test]
    fn scaffold_local_dev_builds_against_workspace() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("local-app");
        let options = ScaffoldOptions {
            name: "local-app".to_string(),
            template: Template::Dashboard,
            target_dir: target.clone(),
            app_id: None,
            local_dev: true,
        };

        run(&options).unwrap();

        let cargo = fs::read_to_string(target.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("crates/kael"));
        assert!(cargo.contains("crates/kael_ui"));
    }

    #[test]
    fn scaffold_refuses_nonempty_target() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("occupied");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("file.txt"), "x").unwrap();
        let options = ScaffoldOptions {
            name: "demo".to_string(),
            template: Template::Messaging,
            target_dir: target,
            app_id: None,
            local_dev: false,
        };
        assert!(run(&options).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn scaffold_refuses_symlink_target() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().unwrap();
        let destination = tmp.path().join("destination");
        let target = tmp.path().join("linked");
        fs::create_dir(&destination).unwrap();
        symlink(&destination, &target).unwrap();
        let options = ScaffoldOptions {
            name: "demo".to_string(),
            template: Template::Dashboard,
            target_dir: target,
            app_id: None,
            local_dev: false,
        };
        assert!(run(&options).is_err());
        assert!(fs::read_dir(destination).unwrap().next().is_none());
    }

    #[test]
    fn concurrent_scaffolds_do_not_delete_the_winner() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("raced");
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let target = target.clone();
                std::thread::spawn(move || {
                    run(&ScaffoldOptions {
                        name: "raced".to_string(),
                        template: Template::Dashboard,
                        target_dir: target,
                        app_id: Some("com.kael.raced".to_string()),
                        local_dev: false,
                    })
                })
            })
            .collect();
        let successes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(Result::is_ok)
            .count();
        assert_eq!(successes, 1);
        assert!(target.join("Cargo.toml").is_file());
        assert!(target.join("src/main.rs").is_file());
        assert!(target.join("kael.dist.toml").is_file());
        assert!(target.join("README.md").is_file());
    }

    /// Scaffolds with `--local-dev` and runs a real `cargo check` against the
    /// in-tree crates. Ignored by default because the nested build is slow and
    /// may need to populate Cargo's dependency cache.
    #[test]
    #[ignore = "runs a full nested cargo check; slow"]
    fn scaffold_local_dev_cargo_check() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("checked-app");
        let options = ScaffoldOptions {
            name: "checked-app".to_string(),
            template: Template::Dashboard,
            target_dir: target.clone(),
            app_id: None,
            local_dev: true,
        };
        run(&options).unwrap();

        let status = std::process::Command::new(env!("CARGO"))
            .current_dir(&target)
            .args(["check", "--features", "kael/runtime_shaders"])
            .status()
            .expect("spawning cargo check");
        assert!(status.success(), "scaffolded app failed cargo check");
    }
}
