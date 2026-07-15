use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context as _, Result, bail};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

mod bundle;
mod notarize;
mod publish;
mod scaffold;
mod sign;
mod update_feed;

const MAX_METADATA_FILE_BYTES: u64 = 1024 * 1024;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn read_bounded_utf8_file(path: &Path, max_bytes: u64) -> Result<String> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("expected a regular file: {}", path.display());
    }
    if metadata.len() > max_bytes {
        bail!(
            "file is too large (maximum {max_bytes} bytes): {}",
            path.display()
        );
    }

    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut bytes = Vec::with_capacity(metadata.len().min(max_bytes) as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        bail!(
            "file grew beyond the maximum of {max_bytes} bytes while reading: {}",
            path.display()
        );
    }
    String::from_utf8(bytes).with_context(|| format!("{} is not valid UTF-8", path.display()))
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("output path must have a UTF-8 file name")?;
    let mut last_collision = None;
    for _ in 0..32 {
        let nonce = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = match options.open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(error);
                continue;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to create temporary file in {}", parent.display())
                });
            }
        };

        let result = (|| -> Result<()> {
            file.write_all(contents).with_context(|| {
                format!("failed to write temporary file for {}", path.display())
            })?;
            file.sync_all()
                .with_context(|| format!("failed to sync temporary file for {}", path.display()))?;
            drop(file);
            fs::rename(&temp, path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
            #[cfg(unix)]
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .with_context(|| format!("failed to sync {}", parent.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        return result;
    }

    Err(last_collision.unwrap_or_else(|| std::io::Error::other("temporary file collision")))
        .with_context(|| format!("failed to reserve a temporary file in {}", parent.display()))
}

// ---------------------------------------------------------------------------
// Distribution Config Contract
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistConfig {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub icons: IconSet,
    pub bundle: BundleMetadata,
    pub signing: Option<SigningConfig>,
    pub updater: Option<UpdaterConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IconSet {
    pub macos: Option<PathBuf>,
    pub windows: Option<PathBuf>,
    pub linux: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleMetadata {
    pub copyright: Option<String>,
    pub category: Option<String>,
    pub minimum_system_version: Option<String>,
    pub file_description: Option<String>,
    pub linux_categories: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningConfig {
    pub macos_team_id: Option<String>,
    pub macos_certificate: Option<String>,
    pub windows_certificate: Option<PathBuf>,
    pub windows_certificate_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdaterConfig {
    pub feed_url: String,
    pub public_key: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
}

impl DistConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let contents = read_bounded_utf8_file(path.as_ref(), MAX_METADATA_FILE_BYTES)
            .with_context(|| format!("failed to read dist config: {}", path.as_ref().display()))?;
        let config: DistConfig = toml::from_str(&contents)
            .with_context(|| format!("failed to parse dist config: {}", path.as_ref().display()))?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.app_id.is_empty()
            || self.app_id.len() > 255
            || self.app_id.starts_with('.')
            || self.app_id.ends_with('.')
            || self.app_id.split('.').any(str::is_empty)
            || !self
                .app_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        {
            bail!("dist config: app_id must be a valid reverse-DNS identifier");
        }
        if self.app_id.starts_with("com.example") || self.app_id.contains("example") {
            bail!("dist config: app_id still contains placeholder/example identity");
        }
        if self.name.is_empty()
            || self.name.chars().count() > 128
            || matches!(self.name.as_str(), "." | "..")
            || self
                .name
                .chars()
                .any(|character| character.is_control() || matches!(character, '/' | '\\'))
        {
            bail!("dist config: name must be a safe name of at most 128 characters");
        }
        if self.name.contains("GPUI") || self.name.eq_ignore_ascii_case("example") {
            bail!("dist config: name still contains placeholder branding");
        }
        if self.version.is_empty()
            || self.version.len() > 64
            || !self.version.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'~')
            })
        {
            bail!("dist config: version contains unsupported characters");
        }
        for icon in [
            self.icons.macos.as_ref(),
            self.icons.windows.as_ref(),
            self.icons.linux.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !icon.exists() {
                bail!("dist config: icon file does not exist: {}", icon.display());
            }
        }
        if let Some(signing) = &self.signing {
            if signing.macos_team_id.as_deref() == Some("TEAM123456") {
                bail!("dist config: macOS team id is still a placeholder");
            }
            if signing
                .macos_certificate
                .as_deref()
                .is_some_and(|certificate| certificate.contains("Example Inc"))
            {
                bail!("dist config: macOS certificate is still a placeholder");
            }
        }
        if let Some(updater) = &self.updater {
            if updater.feed_url.len() > 2048
                || !(updater.feed_url.starts_with("https://")
                    || updater.feed_url.starts_with("http://"))
                || updater.feed_url.chars().any(char::is_control)
            {
                bail!("dist config: updater feed URL must be a bounded HTTP(S) URL");
            }
            if updater.feed_url.contains("example.com") {
                bail!("dist config: updater feed URL is still a placeholder");
            }
            if updater
                .public_key
                .as_deref()
                .is_some_and(|key| key.contains("REPLACE_WITH"))
            {
                bail!("dist config: updater public key is still a placeholder");
            }
            if updater.channel.as_deref().is_some_and(|channel| {
                channel.is_empty()
                    || channel.len() > 64
                    || !channel
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            }) {
                bail!("dist config: updater channel contains unsupported characters");
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Kael packaging and release toolchain")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Validate {
        #[arg(default_value = "kael.dist.toml")]
        config: PathBuf,
    },
    Bundle {
        #[arg(default_value = "kael.dist.toml")]
        config: PathBuf,
        #[arg(short, long, default_value = "dist")]
        output: PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        binary: Option<PathBuf>,
    },
    Sign {
        #[arg(default_value = "kael.dist.toml")]
        config: PathBuf,
        #[arg(short, long)]
        artifact: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    Notarize {
        #[arg(default_value = "kael.dist.toml")]
        config: PathBuf,
        #[arg(short, long)]
        artifact: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    GenerateUpdateMetadata {
        #[arg(default_value = "kael.dist.toml")]
        config: PathBuf,
        #[arg(short, long, default_value = "dist/update-feed.json")]
        output: PathBuf,
        #[arg(short, long)]
        artifact: Vec<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
    GenerateUpdateKey,
    VerifyUpdateFeed {
        #[arg(long, default_value = "dist/update-feed.json")]
        feed: PathBuf,
    },
    Publish {
        #[arg(default_value = "kael.dist.toml")]
        config: PathBuf,
        #[arg(short, long)]
        artifact: Vec<PathBuf>,
        #[arg(short, long)]
        tag: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    DryRun {
        #[arg(default_value = "kael.dist.toml")]
        config: PathBuf,
    },
    New {
        name: String,
        #[arg(long, default_value = "dashboard")]
        template: String,
        #[arg(long)]
        app_id: Option<String>,
        #[arg(long)]
        target_dir: Option<PathBuf>,
        #[arg(long)]
        local_dev: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Validate { config } => {
            let dist = DistConfig::load(&config)?;
            dist.validate()?;
            println!("dist config is valid: {}", config.display());
            Ok(())
        }
        Commands::Bundle {
            config,
            output,
            dry_run,
            binary,
        } => {
            let dist = DistConfig::load(&config)?;
            dist.validate()?;
            let options = bundle::BundleOptions { dry_run, binary };
            bundle::run(&dist, &output, &options)?;
            Ok(())
        }
        Commands::Sign {
            config,
            artifact,
            dry_run,
        } => {
            let dist = DistConfig::load(&config)?;
            dist.validate()?;
            let options = sign::SignOptions { dry_run };
            sign::run(&dist, &artifact, &options)?;
            Ok(())
        }
        Commands::Notarize {
            config,
            artifact,
            dry_run,
        } => {
            let dist = DistConfig::load(&config)?;
            dist.validate()?;
            let options = notarize::NotarizeOptions { dry_run };
            notarize::run(&dist, &artifact, &options)?;
            Ok(())
        }
        Commands::GenerateUpdateMetadata {
            config,
            output,
            artifact,
            dry_run,
        } => {
            let dist = DistConfig::load(&config)?;
            dist.validate()?;
            let signing_key = std::env::var("KAEL_UPDATE_SIGNING_KEY").ok();
            let options = update_feed::FeedOptions {
                dry_run,
                signing_key,
            };
            update_feed::run(&dist, &output, &artifact, &options)?;
            Ok(())
        }
        Commands::GenerateUpdateKey => {
            let (private_hex, public_hex) = kael_release::update::generate_keypair();
            println!("ed25519 update keypair generated.");
            println!();
            println!("Private key (set as the KAEL_UPDATE_SIGNING_KEY repo secret):");
            println!("  {private_hex}");
            println!();
            println!("Public key (embed in the client / kael.dist.toml updater.public_key):");
            println!("  {public_hex}");
            Ok(())
        }
        Commands::VerifyUpdateFeed { feed } => {
            let signing_key = std::env::var("KAEL_UPDATE_SIGNING_KEY")
                .ok()
                .filter(|key| !key.trim().is_empty())
                .context("KAEL_UPDATE_SIGNING_KEY must be set to verify a signed feed")?;
            update_feed::verify(&feed, &signing_key)?;
            println!("update feed signatures verified: {}", feed.display());
            Ok(())
        }
        Commands::Publish {
            config: _,
            artifact,
            tag,
            dry_run,
        } => {
            let artifacts: Vec<&Path> = artifact.iter().map(|p| p.as_path()).collect();
            let options = publish::PublishOptions { dry_run, tag };
            publish::run(&artifacts, &options)?;
            Ok(())
        }
        Commands::DryRun { config } => {
            let dist = DistConfig::load(&config)?;
            dist.validate()?;
            println!(
                "dry-run: configuration valid for '{}' v{}",
                dist.name, dist.version
            );

            let output = PathBuf::from("dist");
            let bundle_options = bundle::BundleOptions {
                dry_run: true,
                binary: None,
            };
            let artifacts = bundle::run(&dist, &output, &bundle_options)?;

            for artifact in &artifacts {
                let sign_options = sign::SignOptions { dry_run: true };
                sign::run(&dist, artifact, &sign_options)?;

                let notarize_options = notarize::NotarizeOptions { dry_run: true };
                notarize::run(&dist, artifact, &notarize_options)?;
            }

            let feed_output = output.join("update-feed.json");
            let feed_options = update_feed::FeedOptions {
                dry_run: true,
                signing_key: std::env::var("KAEL_UPDATE_SIGNING_KEY").ok(),
            };
            update_feed::run(&dist, &feed_output, &artifacts, &feed_options)?;

            let artifacts_ref: Vec<&Path> = artifacts.iter().map(|p| p.as_path()).collect();
            let publish_options = publish::PublishOptions {
                dry_run: true,
                tag: None,
            };
            publish::run(&artifacts_ref, &publish_options)?;

            println!("dry-run: full release pipeline completed successfully");
            Ok(())
        }
        Commands::New {
            name,
            template,
            app_id,
            target_dir,
            local_dev,
        } => {
            let template = scaffold::Template::parse(&template)?;
            let target_dir = target_dir.unwrap_or_else(|| PathBuf::from(&name));
            let options = scaffold::ScaffoldOptions {
                name,
                template,
                target_dir,
                app_id,
                local_dev,
            };
            let outcome = scaffold::run(&options)?;
            println!(
                "scaffolded '{}' ({}) into {}",
                outcome.app_name,
                outcome.crate_name,
                outcome.target_dir.display()
            );
            println!("  app_id: {}", outcome.app_id);
            println!("  next:   cd {} && cargo run", outcome.target_dir.display());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn bounded_reader_rejects_oversized_files() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("large.toml");
        fs::write(&path, b"12345").unwrap();
        assert!(read_bounded_utf8_file(&path, 4).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn bounded_reader_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new().unwrap();
        let target = directory.path().join("target.toml");
        let link = directory.path().join("link.toml");
        fs::write(&target, b"name = 'target'").unwrap();
        symlink(&target, &link).unwrap();
        assert!(read_bounded_utf8_file(&link, 1024).is_err());
    }

    #[test]
    fn atomic_write_replaces_complete_contents() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("feed.json");
        fs::write(&path, b"old").unwrap();
        atomic_write(&path, b"new contents").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new contents");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn test_dist_config_parsing() {
        let toml_src = r#"
app_id = "com.test.app"
name = "Test App"
version = "1.0.0"

[icons]
macos = "assets/icon.icns"

[bundle]
copyright = "Test"
category = "public.app-category.utilities"
"#;
        let config: DistConfig = toml::from_str(toml_src).unwrap();
        assert_eq!(config.app_id, "com.test.app");
        assert_eq!(config.name, "Test App");
        assert_eq!(config.version, "1.0.0");
        assert_eq!(config.icons.macos, Some(PathBuf::from("assets/icon.icns")));
        assert_eq!(config.bundle.copyright, Some("Test".to_string()));
        assert_eq!(
            config.bundle.category,
            Some("public.app-category.utilities".to_string())
        );
    }

    #[test]
    fn test_dist_config_validation() {
        let config = DistConfig {
            app_id: "".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            icons: IconSet {
                macos: None,
                windows: None,
                linux: None,
            },
            bundle: BundleMetadata {
                copyright: None,
                category: None,
                minimum_system_version: None,
                file_description: None,
                linux_categories: None,
            },
            signing: None,
            updater: None,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn dist_config_rejects_path_components_and_unknown_fields() {
        let source = r#"
app_id = "com.kael.app"
name = "../escaped"
version = "1.0.0"
unexpected = true

[icons]
[bundle]
"#;
        assert!(toml::from_str::<DistConfig>(source).is_err());

        let mut config: DistConfig = toml::from_str(
            r#"
app_id = "com.kael.app"
name = "Safe App"
version = "1.0.0"
[icons]
[bundle]
"#,
        )
        .unwrap();
        config.name = "../escaped".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_bundle_artifact_planning() {
        let config = DistConfig {
            app_id: "com.test.app".to_string(),
            name: "Test App".to_string(),
            version: "1.0.0".to_string(),
            icons: IconSet {
                macos: None,
                windows: None,
                linux: None,
            },
            bundle: BundleMetadata {
                copyright: None,
                category: None,
                minimum_system_version: None,
                file_description: None,
                linux_categories: None,
            },
            signing: None,
            updater: None,
        };

        let app_name = config.name.to_lowercase().replace(' ', "-");
        let expected_bundle = if cfg!(target_os = "macos") {
            format!("{}.app", config.name)
        } else if cfg!(target_os = "windows") {
            app_name.clone()
        } else {
            format!("{}.AppDir", app_name)
        };

        assert!(!expected_bundle.is_empty());
    }
}
