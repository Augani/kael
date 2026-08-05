//! Share services for Kael applications.

#![deny(missing_docs)]

mod platform;

use anyhow::{Result, bail};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc, time::Duration};

pub use platform::PlatformShareSupport;

type ReceiverCallback = Box<dyn Fn(Vec<ShareItem>) + Send + 'static>;

const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TOTAL_IMAGE_BYTES: usize = 256 * 1024 * 1024;

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

    /// Returns whether the image has a suggested file name.
    pub fn has_suggested_name(&self) -> bool {
        self.suggested_name
            .as_deref()
            .is_some_and(|name| !name.is_empty())
    }

    /// Returns the number of payload bytes.
    pub fn len_bytes(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether the image payload is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Returns a coarse payload size class without exposing exact bytes.
    pub fn byte_size_class(&self) -> &'static str {
        share_byte_size_class(self.bytes.len())
    }

    /// Human-readable, content-safe summary for logs and agents.
    pub fn to_text(&self) -> String {
        format!(
            "share image: mime {}, bytes {}, suggested name {}",
            self.mime_type_class(),
            self.byte_size_class(),
            self.has_suggested_name()
        )
    }

    fn mime_type_class(&self) -> &'static str {
        if self.mime_type.starts_with("image/") {
            "image"
        } else if self.mime_type.is_empty() {
            "empty"
        } else {
            "other"
        }
    }

    #[cfg(any(target_os = "linux", test))]
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

    /// Creates a share item containing plain text.
    pub fn text(text: impl Into<String>) -> Self {
        Self::new().with_text(text)
    }

    /// Creates a share item containing a URL.
    pub fn url(url: impl Into<String>) -> Self {
        Self::new().with_url(url)
    }

    /// Creates a share item containing one file attachment.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new().with_file(path)
    }

    /// Creates a share item containing multiple file attachments.
    pub fn files<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        Self::new().with_files(paths)
    }

    /// Creates a share item containing an in-memory image.
    pub fn image(image: ShareImage) -> Self {
        Self::new().with_image(image)
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
    }

    /// Returns whether this item contains a non-empty text payload.
    pub fn has_text(&self) -> bool {
        self.text.as_deref().is_some_and(|text| !text.is_empty())
    }

    /// Returns whether this item contains a non-empty URL payload.
    pub fn has_url(&self) -> bool {
        self.url.as_deref().is_some_and(|url| !url.is_empty())
    }

    /// Returns whether this item contains an image payload.
    pub fn has_image(&self) -> bool {
        self.image.is_some()
    }

    /// Returns whether this item contains a non-empty subject line.
    pub fn has_subject(&self) -> bool {
        self.subject
            .as_deref()
            .is_some_and(|subject| !subject.is_empty())
    }

    /// Number of file attachments on this item.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Human-readable, content-safe summary for logs and agents.
    pub fn to_text(&self) -> String {
        format!(
            "share item: text {}, url {}, files {}, image {}, subject {}",
            self.has_text(),
            self.has_url(),
            self.file_count(),
            self.has_image(),
            self.has_subject()
        )
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
    /// Stable destination-family key for logs, settings, and agents.
    pub fn to_text(self) -> &'static str {
        self.activity_name()
    }

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

impl ShareResult {
    /// Whether the platform backend accepted a share request.
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }

    /// Whether no share activity was launched.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    /// Whether a concrete platform activity name is available.
    pub fn has_activity_type(&self) -> bool {
        matches!(
            self,
            Self::Completed { activity_type } if !activity_type.is_empty()
        )
    }

    /// Human-readable, content-safe result summary for logs and agents.
    pub fn to_text(&self) -> String {
        match self {
            Self::Completed { activity_type } => {
                format!(
                    "share result: completed true, activity {}",
                    !activity_type.is_empty()
                )
            }
            Self::Cancelled => "share result: completed false, activity false".to_string(),
        }
    }
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

    /// Creates a share sheet containing plain text.
    pub fn text(text: impl Into<String>) -> Self {
        Self::new(vec![ShareItem::text(text)])
    }

    /// Creates a share sheet containing one URL.
    pub fn url(url: impl Into<String>) -> Self {
        Self::new(vec![ShareItem::url(url)])
    }

    /// Creates a share sheet containing one file attachment.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(vec![ShareItem::file(path)])
    }

    /// Creates a share sheet containing multiple file attachments.
    pub fn files<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        Self::new(vec![ShareItem::files(paths)])
    }

    /// Creates a builder for composing share payloads incrementally.
    pub fn builder() -> ShareSheetBuilder {
        ShareSheetBuilder::new()
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

    /// Number of configured share items.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Number of items containing plain text.
    pub fn text_item_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.text.as_deref().is_some_and(|text| !text.is_empty()))
            .count()
    }

    /// Number of items containing URLs.
    pub fn url_item_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.url.as_deref().is_some_and(|url| !url.is_empty()))
            .count()
    }

    /// Number of file attachments across all items.
    pub fn file_attachment_count(&self) -> usize {
        self.items
            .iter()
            .map(|item| item.files.len())
            .fold(0, usize::saturating_add)
    }

    /// Number of in-memory image payloads.
    pub fn image_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.image.is_some())
            .count()
    }

    /// Number of excluded destination families.
    pub fn excluded_type_count(&self) -> usize {
        self.excluded_types.len()
    }

    /// Whether any item includes a subject line.
    pub fn has_subject(&self) -> bool {
        self.items.iter().any(|item| {
            item.subject
                .as_deref()
                .is_some_and(|subject| !subject.is_empty())
        })
    }

    /// Human-readable, deterministic summary for export/share logs and agents.
    pub fn to_text(&self) -> String {
        format!(
            "share sheet: {} items, {} text, {} urls, {} files, {} images, {} excluded types, subject {}",
            self.item_count(),
            self.text_item_count(),
            self.url_item_count(),
            self.file_attachment_count(),
            self.image_count(),
            self.excluded_type_count(),
            self.has_subject()
        )
    }

    /// Content-safe summaries for each item in this share sheet.
    pub fn item_summaries(&self) -> Vec<String> {
        self.items.iter().map(ShareItem::to_text).collect()
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

    #[cfg(test)]
    pub(crate) fn all_urls(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter_map(|item| item.url.as_deref())
            .filter(|url| !url.is_empty())
            .collect()
    }

    #[cfg(any(target_os = "linux", test))]
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

    #[cfg(any(target_os = "windows", test))]
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

    /// Validate the configured payloads before invoking a platform backend.
    pub fn validate(&self) -> Result<()> {
        const MAX_ITEMS: usize = 256;
        const MAX_FILES: usize = 256;
        const MAX_TEXT_BYTES: usize = 1024 * 1024;
        const MAX_TOTAL_TEXT_BYTES: usize = 8 * 1024 * 1024;

        if self.items.is_empty() {
            bail!("share sheet requires at least one item");
        }
        if self.items.len() > MAX_ITEMS {
            bail!("share sheet contains more than {MAX_ITEMS} items");
        }

        if self.items.iter().all(ShareItem::is_empty) {
            bail!("share sheet requires at least one non-empty payload");
        }

        let mut total_text_bytes = 0usize;
        let mut total_files = 0usize;
        let mut total_image_bytes = 0usize;
        for item in &self.items {
            for value in [item.text.as_deref(), item.subject.as_deref()]
                .into_iter()
                .flatten()
            {
                if value.len() > MAX_TEXT_BYTES {
                    bail!("share text or subject exceeds the {MAX_TEXT_BYTES} byte limit");
                }
                total_text_bytes = total_text_bytes
                    .checked_add(value.len())
                    .ok_or_else(|| anyhow::anyhow!("share text size overflow"))?;
            }
            if let Some(url) = item.url.as_deref().filter(|url| !url.is_empty()) {
                if url.len() > 8_192 || url.chars().any(char::is_control) {
                    bail!("share URL is too large or contains control characters");
                }
                url::Url::parse(url).map_err(|_| anyhow::anyhow!("share URL is invalid"))?;
                total_text_bytes = total_text_bytes
                    .checked_add(url.len())
                    .ok_or_else(|| anyhow::anyhow!("share text size overflow"))?;
            }

            if let Some(image) = item.image.as_ref() {
                if !image.mime_type().starts_with("image/")
                    || image.mime_type().len() > 127
                    || image.mime_type().chars().any(char::is_control)
                {
                    bail!("share image MIME type must be a valid image type");
                }
                if image.bytes().is_empty() {
                    bail!("share image bytes cannot be empty");
                }
                if image.bytes().len() > MAX_IMAGE_BYTES {
                    bail!("share image exceeds the {MAX_IMAGE_BYTES} byte limit");
                }
                total_image_bytes =
                    checked_total_image_bytes(total_image_bytes, image.bytes().len())?;
                if let Some(name) = image.suggested_name() {
                    if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
                        bail!("share image suggested name is invalid");
                    }
                }
            }

            total_files = total_files
                .checked_add(item.files.len())
                .ok_or_else(|| anyhow::anyhow!("share attachment count overflow"))?;
            for path in &item.files {
                let metadata = std::fs::metadata(path).map_err(|_| {
                    anyhow::anyhow!(
                        "share file does not exist or cannot be inspected: {}",
                        path.display()
                    )
                })?;
                if !metadata.is_file() {
                    bail!("share attachment is not a regular file: {}", path.display());
                }
            }
        }
        if total_files > MAX_FILES {
            bail!("share sheet contains more than {MAX_FILES} file attachments");
        }
        if total_text_bytes > MAX_TOTAL_TEXT_BYTES {
            bail!("share sheet text exceeds the {MAX_TOTAL_TEXT_BYTES} byte limit");
        }

        Ok(())
    }
}

fn checked_total_image_bytes(current: usize, additional: usize) -> Result<usize> {
    let total = current
        .checked_add(additional)
        .ok_or_else(|| anyhow::anyhow!("share image size overflow"))?;
    if total > MAX_TOTAL_IMAGE_BYTES {
        bail!("share images exceed the {MAX_TOTAL_IMAGE_BYTES} byte total limit");
    }
    Ok(total)
}

/// Builder for composing a checked share sheet from common payload types.
#[derive(Clone, Debug, Default)]
pub struct ShareSheetBuilder {
    items: Vec<ShareItem>,
    pending_subject: Option<String>,
    excluded_types: BTreeSet<ShareType>,
}

impl ShareSheetBuilder {
    /// Creates an empty share sheet builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a plain text item.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.items.push(ShareItem::text(text));
        self
    }

    /// Add a URL item.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.items.push(ShareItem::url(url));
        self
    }

    /// Add a file attachment item.
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.items.push(ShareItem::file(path));
        self
    }

    /// Add a multi-file attachment item.
    pub fn files<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.items.push(ShareItem::files(paths));
        self
    }

    /// Add an in-memory image item.
    pub fn image(mut self, image: ShareImage) -> Self {
        self.items.push(ShareItem::image(image));
        self
    }

    /// Set the subject used by mail-like share targets.
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.pending_subject = Some(subject.into());
        self
    }

    /// Exclude one destination type.
    pub fn exclude(mut self, share_type: ShareType) -> Self {
        self.excluded_types.insert(share_type);
        self
    }

    /// Exclude multiple destination types.
    pub fn exclude_many(mut self, share_types: impl IntoIterator<Item = ShareType>) -> Self {
        self.excluded_types.extend(share_types);
        self
    }

    /// Returns the configured share items.
    pub fn items(&self) -> &[ShareItem] {
        &self.items
    }

    /// Number of configured share items before build-time subject application.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Number of items containing plain text.
    pub fn text_item_count(&self) -> usize {
        self.items.iter().filter(|item| item.has_text()).count()
    }

    /// Number of items containing URLs.
    pub fn url_item_count(&self) -> usize {
        self.items.iter().filter(|item| item.has_url()).count()
    }

    /// Number of file attachments across all configured items.
    pub fn file_attachment_count(&self) -> usize {
        self.items
            .iter()
            .map(ShareItem::file_count)
            .fold(0, usize::saturating_add)
    }

    /// Number of configured image payloads.
    pub fn image_count(&self) -> usize {
        self.items.iter().filter(|item| item.has_image()).count()
    }

    /// Number of excluded destination families.
    pub fn excluded_type_count(&self) -> usize {
        self.excluded_types.len()
    }

    /// Whether the builder has a pending subject to apply while building.
    pub fn has_pending_subject(&self) -> bool {
        self.pending_subject
            .as_deref()
            .is_some_and(|subject| !subject.is_empty())
    }

    /// Human-readable, content-safe summary before building the share sheet.
    pub fn to_text(&self) -> String {
        format!(
            "share sheet builder: {} items, {} text, {} urls, {} files, {} images, {} excluded types, pending subject {}",
            self.item_count(),
            self.text_item_count(),
            self.url_item_count(),
            self.file_attachment_count(),
            self.image_count(),
            self.excluded_type_count(),
            self.has_pending_subject()
        )
    }

    /// Validate and build the share sheet.
    pub fn build_checked(mut self) -> Result<ShareSheet> {
        if let Some(subject) = self.pending_subject.take() {
            if let Some(first_item) = self.items.first_mut() {
                first_item.subject = Some(subject);
            } else {
                self.items.push(ShareItem::new().with_subject(subject));
            }
        }

        let sheet = ShareSheet {
            items: self.items,
            excluded_types: self.excluded_types,
        };
        sheet.validate()?;
        Ok(sheet)
    }
}

fn share_byte_size_class(len: usize) -> &'static str {
    if len == 0 {
        "empty"
    } else if len < 1024 {
        "small"
    } else if len < 1024 * 1024 {
        "medium"
    } else {
        "large"
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
        if file_types.is_empty() {
            bail!("share receiver registration requires at least one file type");
        }
        if file_types.len() > 256
            || file_types.iter().any(|file_type| {
                file_type.as_str().is_empty()
                    || file_type.as_str().len() > 255
                    || file_type.as_str().chars().any(char::is_control)
            })
        {
            bail!("share receiver file types are invalid or exceed the limit");
        }
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
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
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

#[cfg(any(target_os = "linux", test))]
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
        #[cfg(unix)]
        let directory = {
            use std::os::unix::fs::DirBuilderExt as _;
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder
        };
        #[cfg(not(unix))]
        let directory = fs::DirBuilder::new();
        match directory.create(&dir) {
            Ok(()) => {
                let path = dir.join(&final_name);
                let result = (|| {
                    let mut options = fs::OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt as _;
                        options.mode(0o600);
                    }
                    let mut file = options.open(&path).with_context(|| {
                        format!("failed to materialize share image at {}", path.display())
                    })?;
                    file.write_all(image.bytes()).with_context(|| {
                        format!("failed to write share image at {}", path.display())
                    })?;
                    file.flush().with_context(|| {
                        format!("failed to flush share image at {}", path.display())
                    })?;
                    file.sync_all().with_context(|| {
                        format!("failed to sync share image at {}", path.display())
                    })?;
                    Ok::<_, anyhow::Error>(())
                })();
                match result {
                    Ok(()) => return Ok(path),
                    Err(error) => {
                        let _ = fs::remove_dir_all(&dir);
                        return Err(error);
                    }
                }
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

#[cfg(any(target_os = "linux", test))]
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
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    #[test]
    fn share_sheet_rejects_empty_payloads() {
        let error = futures::executor::block_on(ShareSheet::new(vec![ShareItem::new()]).show())
            .expect_err("empty share sheet should fail validation");
        assert!(error.to_string().contains("non-empty payload"));
    }

    #[test]
    fn share_item_convenience_constructors_create_payloads() {
        assert_eq!(ShareItem::text("hello").text.as_deref(), Some("hello"));
        assert_eq!(
            ShareItem::url("https://example.com").url.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            ShareItem::file("/tmp/report.pdf").files,
            vec![PathBuf::from("/tmp/report.pdf")]
        );
        assert_eq!(
            ShareItem::files(["/tmp/a.txt", "/tmp/b.txt"]).files,
            vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")]
        );
        assert!(
            ShareItem::image(ShareImage::new("image/png", vec![1]))
                .image
                .is_some()
        );
    }

    #[test]
    fn share_sheet_builder_builds_checked_payloads() {
        let builder = ShareSheet::builder()
            .subject("Sprint update")
            .text("All checks passed")
            .url("https://example.com/report")
            .exclude(ShareType::Social)
            .exclude_many([ShareType::Print]);

        assert_eq!(
            builder.to_text(),
            "share sheet builder: 2 items, 1 text, 1 urls, 0 files, 0 images, 2 excluded types, pending subject true"
        );
        assert!(!builder.to_text().contains("Sprint update"));
        assert!(!builder.to_text().contains("All checks passed"));
        assert!(!builder.to_text().contains("example.com"));

        let sheet = builder
            .build_checked()
            .expect("share sheet should be valid");

        assert_eq!(sheet.items().len(), 2);
        assert_eq!(sheet.items()[0].subject.as_deref(), Some("Sprint update"));
        assert!(sheet.excluded().contains(&ShareType::Social));
        assert!(sheet.excluded().contains(&ShareType::Print));
        assert_eq!(sheet.item_count(), 2);
        assert_eq!(sheet.text_item_count(), 1);
        assert_eq!(sheet.url_item_count(), 1);
        assert_eq!(sheet.file_attachment_count(), 0);
        assert_eq!(sheet.image_count(), 0);
        assert_eq!(sheet.excluded_type_count(), 2);
        assert!(sheet.has_subject());
        assert_eq!(
            sheet.to_text(),
            "share sheet: 2 items, 1 text, 1 urls, 0 files, 0 images, 2 excluded types, subject true"
        );
        assert_eq!(
            sheet.item_summaries(),
            vec![
                "share item: text true, url false, files 0, image false, subject true",
                "share item: text false, url true, files 0, image false, subject false",
            ]
        );
        assert!(!sheet.item_summaries().join("; ").contains("Sprint update"));
        assert!(!sheet.item_summaries().join("; ").contains("example.com"));
    }

    #[test]
    fn share_payload_summaries_are_content_safe() {
        let image =
            ShareImage::new("image/png", vec![1, 2, 3]).with_suggested_name("private-preview.png");
        assert_eq!(
            image.to_text(),
            "share image: mime image, bytes small, suggested name true"
        );
        assert_eq!(image.len_bytes(), 3);
        assert!(!image.to_text().contains("image/png"));
        assert!(!image.to_text().contains("private-preview"));

        let item = ShareItem::new()
            .with_text("Private report body")
            .with_url("https://example.com/private-report")
            .with_subject("Private subject")
            .with_file("/tmp/private-report.pdf")
            .with_image(image);

        assert_eq!(
            item.to_text(),
            "share item: text true, url true, files 1, image true, subject true"
        );
        assert!(!item.to_text().contains("Private report"));
        assert!(!item.to_text().contains("example.com"));
        assert!(!item.to_text().contains("private-report.pdf"));
        assert!(!item.to_text().contains("Private subject"));
    }

    #[test]
    fn share_support_type_and_result_summaries_are_content_safe() {
        let support = PlatformShareSupport {
            mail: true,
            messages: false,
            airdrop: false,
            clipboard: true,
            social: false,
            print: true,
            receiver_registration: false,
        };

        assert_eq!(support.supported_count(), 3);
        assert!(!support.is_empty());
        assert_eq!(
            support.to_text(),
            "share support: 3 supported, mail true, messages false, airdrop false, clipboard true, social false, print true, receiver false"
        );
        assert_eq!(ShareType::AirDrop.to_text(), "airdrop");

        let completed = ShareResult::Completed {
            activity_type: "com.apple.UIKit.activity.Mail".into(),
        };
        assert!(completed.is_completed());
        assert!(completed.has_activity_type());
        assert_eq!(
            completed.to_text(),
            "share result: completed true, activity true"
        );
        assert!(!completed.to_text().contains("com.apple"));

        let cancelled = ShareResult::Cancelled;
        assert!(cancelled.is_cancelled());
        assert_eq!(
            cancelled.to_text(),
            "share result: completed false, activity false"
        );
    }

    #[test]
    fn share_sheet_builder_rejects_generated_bad_payloads() {
        assert!(ShareSheet::builder().build_checked().is_err());
        assert!(
            ShareSheet::builder()
                .subject("subject without content")
                .build_checked()
                .is_err()
        );
        assert!(ShareSheet::builder().text("").build_checked().is_err());
        assert!(
            ShareSheet::builder()
                .url("example.com/no-scheme")
                .build_checked()
                .is_err()
        );
        assert!(
            ShareSheet::builder()
                .image(ShareImage::new("", vec![1]))
                .build_checked()
                .is_err()
        );
        assert!(
            ShareSheet::builder()
                .image(ShareImage::new("image/png", Vec::<u8>::new()))
                .build_checked()
                .is_err()
        );
        assert!(
            ShareSheet::builder()
                .image(ShareImage::new("text/plain", vec![1]))
                .build_checked()
                .is_err()
        );
        assert!(
            ShareSheet::builder()
                .url("https://example.com/\nsecret")
                .build_checked()
                .is_err()
        );
        assert!(
            ShareSheet::new(vec![ShareItem::text("x"); 257])
                .validate()
                .is_err()
        );
        assert_eq!(
            checked_total_image_bytes(MAX_TOTAL_IMAGE_BYTES - 1, 1).unwrap(),
            MAX_TOTAL_IMAGE_BYTES
        );
        assert!(checked_total_image_bytes(MAX_TOTAL_IMAGE_BYTES, 1).is_err());
    }

    #[test]
    fn directories_are_not_accepted_as_file_attachments() {
        let directory = tempfile::tempdir().unwrap();
        assert!(ShareSheet::file(directory.path()).validate().is_err());
    }

    #[test]
    fn receiver_inputs_and_materialized_names_are_bounded() {
        assert!(ShareReceiver::register(&[], |_| {}).is_err());
        assert!(ShareReceiver::register(&[ShareFileType::new("bad\n")], |_| {}).is_err());
        assert!(sanitize_file_name(&"a".repeat(1_000)).len() <= 200);
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
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

    #[cfg(unix)]
    #[test]
    fn cleanup_does_not_follow_matching_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep"), b"data").unwrap();
        let link = root.path().join("kael-share-link");
        symlink(outside.path(), &link).unwrap();

        assert_eq!(cleanup_share_temps(root.path(), Duration::ZERO), 0);
        assert!(link.exists());
        assert!(outside.path().join("keep").exists());
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
