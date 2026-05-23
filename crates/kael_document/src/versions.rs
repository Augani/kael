//! Persistent version storage for documents.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::autosave;

/// A stored document version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentVersion {
    /// The unique version identifier.
    pub id: String,
    /// When the version was created.
    pub created_at_millis: u64,
    /// The SHA-256 digest of the stored bytes.
    pub digest: String,
    /// The serialized byte length for the version.
    pub size_bytes: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct VersionStore {
    root: PathBuf,
    max_versions: usize,
}

impl VersionStore {
    pub(crate) fn new_in(root: impl AsRef<Path>, max_versions: usize) -> Result<Self> {
        let root = root.as_ref();
        std::fs::create_dir_all(root)
            .with_context(|| format!("failed to create version root {}", root.display()))?;
        let store = Self {
            root: root.join("document_versions"),
            max_versions: max_versions.max(1),
        };
        store.cleanup_temp_files();
        Ok(store)
    }

    fn cleanup_temp_files(&self) {
        if !self.root.exists() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.filter_map(|e| e.ok()) {
                        let sub_path = sub_entry.path();
                        if sub_path.extension().is_some_and(|ext| ext == "tmp") {
                            let _ = std::fs::remove_file(sub_path);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn load(&self, document_key: &str) -> Result<Vec<DocumentVersion>> {
        let metadata_path = self.metadata_path(document_key);
        if !metadata_path.exists() {
            return Ok(Vec::new());
        }

        let json = std::fs::read(&metadata_path).with_context(|| {
            format!(
                "failed to read document version metadata from {}",
                metadata_path.display()
            )
        })?;
        serde_json::from_slice(&json).context("failed to deserialize document version metadata")
    }

    pub(crate) fn record(&self, document_key: &str, bytes: &[u8]) -> Result<DocumentVersion> {
        let document_dir = self.document_dir(document_key);
        std::fs::create_dir_all(&document_dir).with_context(|| {
            format!(
                "failed to create document version directory {}",
                document_dir.display()
            )
        })?;

        let digest = digest_hex(bytes);
        let blob_path = document_dir.join(format!("{digest}.bin"));
        if !blob_path.exists() {
            std::fs::write(&blob_path, bytes).with_context(|| {
                format!(
                    "failed to write document version blob {}",
                    blob_path.display()
                )
            })?;
        }

        let mut versions = VecDeque::from(self.load(document_key)?);
        let timestamp = now_unix_millis();
        let version = DocumentVersion {
            id: format!("{}-{}", timestamp, short_hash(&digest)),
            created_at_millis: timestamp,
            digest,
            size_bytes: bytes.len(),
        };
        versions.push_back(version.clone());

        while versions.len() > self.max_versions {
            if let Some(removed) = versions.pop_front() {
                if versions
                    .iter()
                    .all(|candidate| candidate.digest != removed.digest)
                {
                    let removed_blob = document_dir.join(format!("{}.bin", removed.digest));
                    let _ = std::fs::remove_file(removed_blob);
                }
            }
        }

        let versions = versions.into_iter().collect::<Vec<_>>();
        self.persist_versions(document_key, &versions)?;
        Ok(version)
    }

    pub(crate) fn read(&self, document_key: &str, version: &DocumentVersion) -> Result<Vec<u8>> {
        let versions = self.load(document_key)?;
        if !versions.iter().any(|candidate| candidate.id == version.id) {
            return Err(anyhow!("unknown document version {}", version.id));
        }

        let blob_path = self
            .document_dir(document_key)
            .join(format!("{}.bin", version.digest));
        let bytes = std::fs::read(&blob_path).with_context(|| {
            format!(
                "failed to read document version blob {}",
                blob_path.display()
            )
        })?;
        let actual_digest = digest_hex(&bytes);
        if actual_digest != version.digest {
            return Err(anyhow!(
                "document version digest mismatch for {}",
                version.id
            ));
        }
        Ok(bytes)
    }

    fn persist_versions(&self, document_key: &str, versions: &[DocumentVersion]) -> Result<()> {
        let metadata_path = self.metadata_path(document_key);
        if let Some(parent) = metadata_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create metadata directory {}", parent.display())
            })?;
        }

        let json = serde_json::to_vec(versions)
            .context("failed to serialize document version metadata")?;
        autosave::write_bytes_atomically(&metadata_path, &json)
    }

    fn document_dir(&self, document_key: &str) -> PathBuf {
        self.root
            .join(short_hash(&digest_hex(document_key.as_bytes())))
    }

    fn metadata_path(&self, document_key: &str) -> PathBuf {
        self.document_dir(document_key).join("versions.json")
    }
}

fn digest_hex(bytes: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(bytes.as_ref());
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn short_hash(digest: &str) -> String {
    digest.chars().take(16).collect()
}

fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_rejects_digest_mismatch() {
        let tmp = TempDir::new().unwrap();
        let store = VersionStore::new_in(tmp.path(), 4).unwrap();
        let version = store.record("doc", b"original").unwrap();
        let blob_path = store
            .document_dir("doc")
            .join(format!("{}.bin", version.digest));
        std::fs::write(blob_path, b"tampered").unwrap();

        let error = store.read("doc", &version).unwrap_err();
        assert!(error.to_string().contains("digest mismatch"));
    }

    #[test]
    fn recovers_from_interrupted_blob_write() {
        let tmp = TempDir::new().unwrap();
        let store = VersionStore::new_in(tmp.path(), 4).unwrap();
        store.record("doc", b"good data").unwrap();

        let doc_dir = store.document_dir("doc");
        std::fs::write(doc_dir.join("partial.tmp"), b"corrupt").unwrap();

        let store2 = VersionStore::new_in(tmp.path(), 4).unwrap();
        let versions = store2.load("doc").unwrap();
        assert_eq!(versions.len(), 1);
        let data = store2.read("doc", &versions[0]).unwrap();
        assert_eq!(data, b"good data");
        assert!(!doc_dir.join("partial.tmp").exists());
    }

    #[test]
    fn retention_policy_limits_stored_versions() {
        let tmp = TempDir::new().unwrap();
        let store = VersionStore::new_in(tmp.path(), 3).unwrap();

        for i in 0..5u8 {
            store.record("doc", format!("v{i}").as_bytes()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let versions = store.load("doc").unwrap();
        assert!(versions.len() <= 3);
    }
}
