use anyhow::{Context as _, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// Crates.io version requirement written into scaffolded projects.
const KAEL_VERSION: &str = "0.3";

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

    if options.target_dir.exists() {
        let is_empty = fs::read_dir(&options.target_dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            bail!(
                "target directory already exists and is not empty: {}",
                options.target_dir.display()
            );
        }
    }

    let main_src = template_main_src(options.template);

    let cargo_toml = render_cargo_toml(options.template, &crate_name, options.local_dev);
    let dist_toml = render_dist_toml(&app_name, &app_id);
    let readme = render_readme(&app_name, &crate_name);
    let main_rs = main_src.replace(options.template.source_window_title(), &app_name);

    fs::create_dir_all(options.target_dir.join("src"))
        .with_context(|| format!("creating {}/src", options.target_dir.display()))?;

    write_file(&options.target_dir.join("Cargo.toml"), &cargo_toml)?;
    write_file(&options.target_dir.join("src/main.rs"), &main_rs)?;
    write_file(&options.target_dir.join("kael.dist.toml"), &dist_toml)?;
    write_file(&options.target_dir.join("README.md"), &readme)?;

    Ok(ScaffoldOutcome {
        target_dir: options.target_dir.clone(),
        crate_name,
        app_name,
        app_id,
    })
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

fn sanitize_crate_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("project name must not be empty");
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
         feed_url = \"https://example.com/updates/feed.json\"\n"
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
         Edit `kael.dist.toml` (set a real `app_id`, signing identity, and updater\n\
         feed URL), then build installers with the `xtask` toolchain from the Kael\n\
         repository.\n\
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
        assert!(!toml.contains("kael = \"0.3\""));
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
        assert!(cargo.contains("kael = \"0.3\""));

        let dist = fs::read_to_string(target.join("kael.dist.toml")).unwrap();
        assert!(dist.contains("app_id = \"com.acme.mydash\""));
        assert!(dist.contains("name = \"My Dash\""));

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

    /// Scaffolds with `--local-dev` and runs a real `cargo check --offline`
    /// against the in-tree crates. Ignored by default because the nested build
    /// is slow; run with `cargo test -p xtask -- --ignored`.
    #[test]
    #[ignore = "runs a full nested cargo check; slow"]
    fn scaffold_local_dev_cargo_check_offline() {
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
            .args(["check", "--features", "kael/runtime_shaders", "--offline"])
            .status()
            .expect("spawning cargo check");
        assert!(status.success(), "scaffolded app failed cargo check");
    }
}
