//! Share services for Kael applications.

#![deny(missing_docs)]

mod platform;

use anyhow::{Result, bail};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc, time::Duration};

pub use platform::PlatformShareSupport;

type ReceiverCallback = Box<dyn Fn(Vec<ShareItem>) + Send + 'static>;

/// An in-memory image payload that can be materialized for sharing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareImage {
    mime_type: String,
    bytes: Arc<[u8]>,
    suggested_name: Option<String>,
}

impl ShareImage {
    /// Creates a new shareable image payload.
    pub fn new(mime_type: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            mime_type: mime_type.into(),
            bytes: Arc::<[u8]>::from(bytes.into().into_boxed_slice()),
            suggested_name: None,
        }
    }

    /// Associates a preferred file name with the image when a backend needs a file path.
    pub fn with_suggested_name(mut self, name: impl Into<String>) -> Self {
        self.suggested_name = Some(name.into());
        self
    }

    /// Returns the image MIME type.
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// Returns the image bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the preferred file name, if one was provided.
    pub fn suggested_name(&self) -> Option<&str> {
        self.suggested_name.as_deref()
    }

    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    fn extension(&self) -> &str {
        match self.mime_type.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/tiff" => "tiff",
            _ => "bin",
        }
    }
}

/// A single unit of content to share through the operating system.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShareItem {
    /// Optional plain text body.
    pub text: Option<String>,
    /// Optional URL to include with the shared payload.
    pub url: Option<String>,
    /// Optional in-memory image payload.
    pub image: Option<ShareImage>,
    /// Optional file attachments.
    pub files: Vec<PathBuf>,
    /// Optional mail-like subject line.
    pub subject: Option<String>,
}

impl ShareItem {
    /// Creates an empty share item.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the plain text payload.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Sets the URL payload.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Sets the share subject.
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Sets the in-memory image payload.
    pub fn with_image(mut self, image: ShareImage) -> Self {
        self.image = Some(image);
        self
    }

    /// Adds a file attachment.
    pub fn with_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.files.push(path.into());
        self
    }

    /// Adds multiple file attachments.
    pub fn with_files<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.files.extend(paths.into_iter().map(Into::into));
        self
    }

    /// Returns true when the item has no shareable payload.
    pub fn is_empty(&self) -> bool {
        self.text.as_deref().is_none_or(str::is_empty)
            && self.url.as_deref().is_none_or(str::is_empty)
            && self.image.is_none()
            && self.files.is_empty()
            && self.subject.as_deref().is_none_or(str::is_empty)
    }

    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    fn body_text(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(text) = self.text.as_deref().filter(|text| !text.is_empty()) {
            parts.push(text.to_string());
        }
        if let Some(url) = self.url.as_deref().filter(|url| !url.is_empty()) {
            parts.push(url.to_string());
        }
        (!parts.is_empty()).then(|| parts.join("\n"))
    }
}

/// High-level share destinations that a backend may support.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum ShareType {
    /// Email composition or handoff.
    Mail,
    /// Text message composition or handoff.
    Messages,
    /// AirDrop-style nearby transfer.
    AirDrop,
    /// Copy the share payload to the system clipboard.
    Clipboard,
    /// Social-posting services.
    Social,
    /// System printing.
    Print,
}

impl ShareType {
    fn activity_name(self) -> &'static str {
        match self {
            ShareType::Mail => "mail",
            ShareType::Messages => "messages",
            ShareType::AirDrop => "airdrop",
            ShareType::Clipboard => "clipboard",
            ShareType::Social => "social",
            ShareType::Print => "print",
        }
    }
}

/// The outcome reported by a share backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareResult {
    /// The backend accepted the share request.
    Completed {
        /// The concrete share activity that was launched.
        activity_type: String,
    },
    /// No share activity was launched.
    Cancelled,
}

/// File-type identifier used when registering as a share target.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShareFileType(String);

impl ShareFileType {
    /// Creates a file-type identifier.
    pub fn new(identifier: impl Into<String>) -> Self {
        Self(identifier.into())
    }

    /// Returns the file-type identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ShareFileType {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ShareFileType {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// A share request configured with payloads and excluded destination types.
#[derive(Clone, Debug, Default)]
pub struct ShareSheet {
    items: Vec<ShareItem>,
    excluded_types: BTreeSet<ShareType>,
}

impl ShareSheet {
    /// Creates a share sheet request from one or more items.
    pub fn new(items: Vec<ShareItem>) -> Self {
        Self {
            items,
            excluded_types: BTreeSet::new(),
        }
    }

    /// Excludes specific destination types from backend selection.
    pub fn excluded_types(mut self, types: &[ShareType]) -> Self {
        self.excluded_types.extend(types.iter().copied());
        self
    }

    /// Returns the configured share items.
    pub fn items(&self) -> &[ShareItem] {
        &self.items
    }

    /// Returns the excluded destination types.
    pub fn excluded(&self) -> &BTreeSet<ShareType> {
        &self.excluded_types
    }

    /// Returns the current platform support summary.
    pub fn platform_support(&self) -> PlatformShareSupport {
        platform::support()
    }

    /// Attempts to launch a share operation using the current platform backend.
    pub async fn show(&self) -> Result<ShareResult> {
        self.validate()?;
        platform::show(self).await
    }

    pub(crate) fn is_excluded(&self, share_type: ShareType) -> bool {
        self.excluded_types.contains(&share_type)
    }

    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    pub(crate) fn first_subject(&self) -> Option<&str> {
        self.items
            .iter()
            .filter_map(|item| item.subject.as_deref())
            .find(|subject| !subject.is_empty())
    }

    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    pub(crate) fn body_text(&self) -> Option<String> {
        let parts: Vec<String> = self.items.iter().filter_map(ShareItem::body_text).collect();
        (!parts.is_empty()).then(|| parts.join("\n\n"))
    }

    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    pub(crate) fn all_urls(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter_map(|item| item.url.as_deref())
            .filter(|url| !url.is_empty())
            .collect()
    }

    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    pub(crate) fn attachment_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for item in &self.items {
            paths.extend(item.files.iter().cloned());
            if let Some(image) = item.image.as_ref() {
                paths.push(materialize_image(image)?);
            }
        }
        Ok(paths)
    }

    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    pub(crate) fn mailto_uri(&self) -> Option<String> {
        use url::form_urlencoded;

        let body = self.body_text();
        let subject = self.first_subject();
        if body.is_none() && subject.is_none() {
            return None;
        }

        let mut serializer = form_urlencoded::Serializer::new(String::new());
        if let Some(subject) = subject {
            serializer.append_pair("subject", subject);
        }
        if let Some(body) = body.as_deref() {
            serializer.append_pair("body", body);
        }

        let query = serializer.finish();
        let mut uri = String::from("mailto:");
        if !query.is_empty() {
            uri.push('?');
            uri.push_str(&query);
        }
        Some(uri)
    }

    fn validate(&self) -> Result<()> {
        if self.items.is_empty() {
            bail!("share sheet requires at least one item");
        }

        if self.items.iter().all(ShareItem::is_empty) {
            bail!("share sheet requires at least one non-empty payload");
        }

        for item in &self.items {
            if let Some(url) = item.url.as_deref().filter(|url| !url.is_empty()) {
                if !url.contains(':') {
                    bail!("share URL must include a URI scheme: {url}");
                }
            }

            if let Some(image) = item.image.as_ref() {
                if image.mime_type().is_empty() {
                    bail!("share image MIME type cannot be empty");
                }
                if image.bytes().is_empty() {
                    bail!("share image bytes cannot be empty");
                }
            }

            for path in &item.files {
                if !path.exists() {
                    bail!("share file does not exist: {}", path.display());
                }
            }
        }

        Ok(())
    }
}

/// Registration handle for share-target callbacks.
pub struct ShareReceiver {
    _registration: platform::PlatformShareReceiver,
}

impl ShareReceiver {
    /// Registers the application as a share target for the given file types.
    pub fn register<F>(file_types: &[ShareFileType], callback: F) -> Result<Self>
    where
        F: Fn(Vec<ShareItem>) + Send + 'static,
    {
        let registration = platform::register_receiver(file_types, Box::new(callback))?;
        Ok(Self {
            _registration: registration,
        })
    }
}

/// Removes stale `kael-share-*` temporary directories older than `max_age`.
///
/// Returns the number of directories successfully removed.
pub fn cleanup_share_temps(temp_dir: &std::path::Path, max_age: Duration) -> usize {
    let cutoff = std::time::SystemTime::now()
        .checked_sub(max_age)
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let mut removed = 0;

    let Ok(entries) = std::fs::read_dir(temp_dir) else {
        return 0;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with("kael-share-") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified <= cutoff {
            if std::fs::remove_dir_all(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn materialize_image(image: &ShareImage) -> Result<PathBuf> {
    use anyhow::Context;
    use std::{
        fs,
        io::Write,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    let temp_dir = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = image
        .suggested_name()
        .map(sanitize_file_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("kael-share-image-{stamp}.{}", image.extension()));
    let final_name = if Path::new(&file_name).extension().is_some() {
        file_name
    } else {
        format!("{file_name}.{}", image.extension())
    };
    for attempt in 0..16 {
        let dir = temp_dir.join(format!(
            "kael-share-{stamp}-{}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&dir) {
            Ok(()) => {
                let path = dir.join(&final_name);
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .with_context(|| {
                        format!("failed to materialize share image at {}", path.display())
                    })?;
                file.write_all(image.bytes()).with_context(|| {
                    format!("failed to write share image at {}", path.display())
                })?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to create share temp dir: {}", dir.display())
                });
            }
        }
    }

    anyhow::bail!("failed to create a unique share temp directory")
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn sanitize_file_name(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    #[test]
    fn share_sheet_rejects_empty_payloads() {
        let error = futures::executor::block_on(ShareSheet::new(vec![ShareItem::new()]).show())
            .expect_err("empty share sheet should fail validation");
        assert!(error.to_string().contains("non-empty payload"));
    }

    #[test]
    fn excluded_types_are_tracked() {
        let sheet = ShareSheet::new(vec![ShareItem::new().with_text("hello")])
            .excluded_types(&[ShareType::Mail, ShareType::Clipboard]);
        assert!(sheet.excluded().contains(&ShareType::Mail));
        assert!(sheet.excluded().contains(&ShareType::Clipboard));
        assert!(!sheet.excluded().contains(&ShareType::AirDrop));
    }

    #[test]
    fn mailto_uri_combines_subject_and_body() {
        let sheet = ShareSheet::new(vec![
            ShareItem::new()
                .with_subject("Sprint update")
                .with_text("All checks passed")
                .with_url("https://example.com/report"),
        ]);

        let uri = sheet.mailto_uri().expect("mailto URI should be created");
        assert!(uri.starts_with("mailto:?"));
        assert!(uri.contains("subject=Sprint+update"));
        assert!(uri.contains("body=All+checks+passed%0Ahttps%3A%2F%2Fexample.com%2Freport"));
    }

    #[test]
    fn image_materialization_uses_mime_extension() {
        let image = ShareImage::new("image/png", vec![1, 2, 3]).with_suggested_name("preview");
        let path = materialize_image(&image).expect("image should materialize");
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("png"));
        let parent = path.parent().map(Path::to_path_buf);
        fs::remove_file(&path).ok();
        if let Some(parent) = parent {
            fs::remove_dir(parent).ok();
        }
    }

    #[test]
    fn materialize_image_uses_unique_non_overwriting_paths() {
        let image = ShareImage::new("image/png", vec![1, 2, 3]).with_suggested_name("preview");
        let first = materialize_image(&image).expect("first image should materialize");
        let second = materialize_image(&image).expect("second image should materialize");

        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), vec![1, 2, 3]);
        assert_eq!(fs::read(&second).unwrap(), vec![1, 2, 3]);

        for path in [first, second] {
            let parent = path.parent().map(Path::to_path_buf);
            fs::remove_file(&path).ok();
            if let Some(parent) = parent {
                fs::remove_dir(parent).ok();
            }
        }
    }

    #[test]
    fn helper_collectors_include_urls_and_attachments() {
        let attachment = std::env::temp_dir().join("kael-share-helper-attachment.txt");
        fs::write(&attachment, "share me").expect("attachment should be created");

        let sheet = ShareSheet::new(vec![
            ShareItem::new()
                .with_url("https://example.com")
                .with_file(&attachment)
                .with_image(
                    ShareImage::new("image/png", vec![9, 8, 7])
                        .with_suggested_name("helper-preview"),
                ),
        ]);

        let urls = sheet.all_urls();
        assert_eq!(urls, vec!["https://example.com"]);

        let attachments = sheet
            .attachment_paths()
            .expect("attachments should materialize");
        assert_eq!(attachments.len(), 2);
        assert!(attachments.iter().any(|path| path == &attachment));
        let image_path = attachments
            .iter()
            .find(|path| *path != &attachment)
            .expect("materialized image path should exist")
            .clone();
        assert_eq!(
            image_path.extension().and_then(|ext| ext.to_str()),
            Some("png")
        );

        fs::remove_file(&attachment).ok();
        let image_parent = image_path.parent().map(Path::to_path_buf);
        fs::remove_file(&image_path).ok();
        if let Some(image_parent) = image_parent {
            fs::remove_dir(image_parent).ok();
        }
    }

    #[test]
    fn cleanup_removes_stale_share_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let old_dir = temp.path().join("kael-share-old-123-0");
        std::fs::create_dir(&old_dir).unwrap();
        std::fs::write(old_dir.join("test.png"), b"fake").unwrap();

        let removed = cleanup_share_temps(temp.path(), Duration::from_secs(0));
        assert_eq!(removed, 1);
        assert!(!old_dir.exists());
    }

    #[test]
    fn cleanup_ignores_non_kael_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let other_dir = temp.path().join("some-other-dir");
        std::fs::create_dir(&other_dir).unwrap();

        let removed = cleanup_share_temps(temp.path(), Duration::from_secs(0));
        assert_eq!(removed, 0);
        assert!(other_dir.exists());
    }

    #[test]
    fn cleanup_returns_zero_for_missing_dir() {
        let missing = std::path::Path::new("/tmp/kael-share-nonexistent-test-dir-xyz");
        let removed = cleanup_share_temps(missing, Duration::from_secs(0));
        assert_eq!(removed, 0);
    }

    #[test]
    fn missing_files_fail_validation() {
        let missing = std::env::temp_dir().join("kael-share-missing-file.txt");
        let error = futures::executor::block_on(
            ShareSheet::new(vec![ShareItem::new().with_file(&missing)]).show(),
        )
        .expect_err("missing files should fail validation");
        assert!(error.to_string().contains("does not exist"));
    }
}
