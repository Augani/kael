//! Recent-document persistence.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// A recently opened document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentDocument {
    /// The normalized document path.
    pub path: PathBuf,
    /// When the document was last opened or saved.
    pub last_opened_at_millis: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct RecentDocumentStore {
    path: PathBuf,
    max_entries: usize,
}

impl RecentDocumentStore {
    pub(crate) fn new_in(root: impl AsRef<Path>, max_entries: usize) -> Result<Self> {
        let root = root.as_ref();
        std::fs::create_dir_all(root)
            .with_context(|| format!("failed to create recent document root {}", root.display()))?;
        Ok(Self {
            path: root.join("recent_documents.json"),
            max_entries: max_entries.max(1),
        })
    }

    pub(crate) fn load(&self) -> Result<Vec<RecentDocument>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let json = std::fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read recent documents from {}", self.path.display()))?;
        serde_json::from_str(&json).context("failed to deserialize recent documents")
    }

    pub(crate) fn record(&self, path: &Path) -> Result<()> {
        let mut documents = self.load()?;
        documents.retain(|document| document.path != path);
        documents.insert(
            0,
            RecentDocument {
                path: path.to_path_buf(),
                last_opened_at_millis: now_unix_millis(),
            },
        );
        if documents.len() > self.max_entries {
            documents.truncate(self.max_entries);
        }
        persist_recent_documents(&self.path, &documents)
    }

    pub(crate) fn clear(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error)
                .with_context(|| format!("failed to remove recent documents file {}", self.path.display())),
        }
    }
}

fn persist_recent_documents(path: &Path, documents: &[RecentDocument]) -> Result<()> {
    let temp_path = path.with_extension("tmp");
    let json = serde_json::to_vec_pretty(documents).context("failed to serialize recent documents")?;
    std::fs::write(&temp_path, json)
        .with_context(|| format!("failed to write recent documents to {}", temp_path.display()))?;
    std::fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to finalize recent documents file from {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}