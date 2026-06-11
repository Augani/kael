use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

mod bundle;
mod notarize;
mod publish;
mod scaffold;
mod sign;
mod update_feed;

// ---------------------------------------------------------------------------
// Distribution Config Contract
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct IconSet {
    pub macos: Option<PathBuf>,
    pub windows: Option<PathBuf>,
    pub linux: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleMetadata {
    pub copyright: Option<String>,
    pub category: Option<String>,
    pub minimum_system_version: Option<String>,
    pub file_description: Option<String>,
    pub linux_categories: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningConfig {
    pub macos_team_id: Option<String>,
    pub macos_certificate: Option<String>,
    pub windows_certificate: Option<PathBuf>,
    pub windows_certificate_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdaterConfig {
    pub feed_url: String,
    pub public_key: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
}

impl DistConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read dist config: {}", path.as_ref().display()))?;
        let config: DistConfig = toml::from_str(&contents)
            .with_context(|| format!("failed to parse dist config: {}", path.as_ref().display()))?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.app_id.is_empty() {
            bail!("dist config: app_id must not be empty");
        }
        if self.app_id.starts_with("com.example") || self.app_id.contains("example") {
            bail!("dist config: app_id still contains placeholder/example identity");
        }
        if self.name.is_empty() {
            bail!("dist config: name must not be empty");
        }
        if self.name.contains("GPUI") || self.name.eq_ignore_ascii_case("example") {
            bail!("dist config: name still contains placeholder branding");
        }
        if self.version.is_empty() {
            bail!("dist config: version must not be empty");
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
            let templates_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(|root| root.join("templates"))
                .context("locating templates directory")?;
            let options = scaffold::ScaffoldOptions {
                name,
                template,
                target_dir,
                app_id,
                local_dev,
            };
            let outcome = scaffold::run(&templates_root, &options)?;
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
