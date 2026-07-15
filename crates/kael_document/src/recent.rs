//! Recent-document persistence.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::autosave;

const MAX_RECENT_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RECENT_METADATA_ENTRIES: usize = 10_000;

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

        let metadata = std::fs::metadata(&self.path).with_context(|| {
            format!(
                "failed to inspect recent documents at {}",
                self.path.display()
            )
        })?;
        anyhow::ensure!(
            metadata.len() <= MAX_RECENT_METADATA_BYTES,
            "recent document metadata exceeds the {MAX_RECENT_METADATA_BYTES} byte limit"
        );
        let json = std::fs::read(&self.path).with_context(|| {
            format!(
                "failed to read recent documents from {}",
                self.path.display()
            )
        })?;
        anyhow::ensure!(
            u64::try_from(json.len()).unwrap_or(u64::MAX) <= MAX_RECENT_METADATA_BYTES,
            "recent document metadata exceeds the {MAX_RECENT_METADATA_BYTES} byte limit"
        );
        let mut documents: Vec<RecentDocument> =
            serde_json::from_slice(&json).context("failed to deserialize recent documents")?;
        anyhow::ensure!(
            documents.len() <= MAX_RECENT_METADATA_ENTRIES,
            "recent document metadata contains more than {MAX_RECENT_METADATA_ENTRIES} entries"
        );
        let mut paths = HashSet::with_capacity(documents.len());
        for document in &documents {
            anyhow::ensure!(
                !document.path.as_os_str().is_empty(),
                "recent document contains an empty path"
            );
            anyhow::ensure!(
                paths.insert(document.path.as_path()),
                "recent document metadata contains duplicate paths"
            );
        }
        documents.sort_by(|left, right| {
            right
                .last_opened_at_millis
                .cmp(&left.last_opened_at_millis)
                .then_with(|| left.path.cmp(&right.path))
        });
        documents.truncate(self.max_entries);
        Ok(documents)
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
            Err(error) => Err(error).with_context(|| {
                format!(
                    "failed to remove recent documents file {}",
                    self.path.display()
                )
            }),
        }
    }
}

fn persist_recent_documents(path: &Path, documents: &[RecentDocument]) -> Result<()> {
    anyhow::ensure!(
        documents.len() <= MAX_RECENT_METADATA_ENTRIES,
        "recent document metadata contains more than {MAX_RECENT_METADATA_ENTRIES} entries"
    );
    let json = serde_json::to_vec(documents).context("failed to serialize recent documents")?;
    anyhow::ensure!(
        u64::try_from(json.len()).unwrap_or(u64::MAX) <= MAX_RECENT_METADATA_BYTES,
        "recent document metadata exceeds the {MAX_RECENT_METADATA_BYTES} byte limit"
    );
    autosave::write_bytes_atomically(path, &json)
}

fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_sorts_truncates_and_rejects_duplicate_paths() {
        let directory = tempfile::tempdir().unwrap();
        let store = RecentDocumentStore::new_in(directory.path(), 2).unwrap();
        let documents = vec![
            RecentDocument {
                path: PathBuf::from("older"),
                last_opened_at_millis: 1,
            },
            RecentDocument {
                path: PathBuf::from("newest"),
                last_opened_at_millis: 3,
            },
            RecentDocument {
                path: PathBuf::from("middle"),
                last_opened_at_millis: 2,
            },
        ];
        std::fs::write(&store.path, serde_json::to_vec(&documents).unwrap()).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].path, Path::new("newest"));
        assert_eq!(loaded[1].path, Path::new("middle"));

        let duplicate = vec![documents[0].clone(), documents[0].clone()];
        std::fs::write(&store.path, serde_json::to_vec(&duplicate).unwrap()).unwrap();
        assert!(store.load().is_err());
    }
}
