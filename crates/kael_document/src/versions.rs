//! Persistent version storage for documents.

use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::autosave;

const MAX_VERSION_METADATA_BYTES: u64 = 16 * 1024 * 1024;
const MAX_VERSION_METADATA_ENTRIES: usize = 10_000;

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
        autosave::ensure_real_directory(root, "document metadata root")?;
        let store = Self {
            root: root.join("document_versions"),
            max_versions: max_versions.max(1),
        };
        autosave::ensure_real_directory(&store.root, "document version root")?;
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
            if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
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

        let json = autosave::read_regular_file_bounded(
            &metadata_path,
            MAX_VERSION_METADATA_BYTES,
            "document version metadata",
        )?;
        let versions: Vec<DocumentVersion> = serde_json::from_slice(&json)
            .context("failed to deserialize document version metadata")?;
        validate_versions(&versions)?;
        Ok(versions)
    }

    pub(crate) fn record(&self, document_key: &str, bytes: &[u8]) -> Result<DocumentVersion> {
        let document_dir = self.document_dir(document_key);
        autosave::ensure_real_directory(&document_dir, "document version directory")?;

        let mut versions = VecDeque::from(self.load(document_key)?);
        let digest = digest_hex(bytes);
        let blob_path = document_dir.join(format!("{digest}.bin"));
        autosave::write_private_bytes_atomically(&blob_path, bytes).with_context(|| {
            format!(
                "failed to write document version blob {}",
                blob_path.display()
            )
        })?;

        if let Some(existing) = versions.back()
            && existing.digest == digest
        {
            return Ok(existing.clone());
        }
        let timestamp = versions.back().map_or_else(now_unix_millis, |last| {
            now_unix_millis().max(last.created_at_millis)
        });
        let version = DocumentVersion {
            id: next_version_id(timestamp, &digest, &versions),
            created_at_millis: timestamp,
            digest,
            size_bytes: bytes.len(),
        };
        versions.push_back(version.clone());

        let mut stale_digests = Vec::new();
        while versions.len() > self.max_versions {
            if let Some(removed) = versions.pop_front() {
                if versions
                    .iter()
                    .all(|candidate| candidate.digest != removed.digest)
                {
                    stale_digests.push(removed.digest);
                }
            }
        }

        let versions = versions.into_iter().collect::<Vec<_>>();
        self.persist_versions(document_key, &versions)?;
        for digest in stale_digests {
            let _ = std::fs::remove_file(document_dir.join(format!("{digest}.bin")));
        }
        Ok(version)
    }

    pub(crate) fn read(&self, document_key: &str, version: &DocumentVersion) -> Result<Vec<u8>> {
        let versions = self.load(document_key)?;
        let stored = versions
            .iter()
            .find(|candidate| candidate.id == version.id)
            .ok_or_else(|| anyhow!("unknown document version {}", version.id))?;

        let blob_path = self
            .document_dir(document_key)
            .join(format!("{}.bin", stored.digest));
        let bytes = autosave::read_regular_file_bounded(
            &blob_path,
            autosave::MAX_DOCUMENT_BYTES,
            "document version blob",
        )?;
        let actual_digest = digest_hex(&bytes);
        if actual_digest != stored.digest {
            return Err(anyhow!(
                "document version digest mismatch for {}",
                stored.id
            ));
        }
        anyhow::ensure!(
            bytes.len() == stored.size_bytes,
            "document version size mismatch for {}",
            stored.id
        );
        Ok(bytes)
    }

    fn persist_versions(&self, document_key: &str, versions: &[DocumentVersion]) -> Result<()> {
        validate_versions(versions)?;
        let metadata_path = self.metadata_path(document_key);
        if let Some(parent) = metadata_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create metadata directory {}", parent.display())
            })?;
        }

        let json = serde_json::to_vec(versions)
            .context("failed to serialize document version metadata")?;
        anyhow::ensure!(
            u64::try_from(json.len()).unwrap_or(u64::MAX) <= MAX_VERSION_METADATA_BYTES,
            "document version metadata exceeds the {MAX_VERSION_METADATA_BYTES} byte limit"
        );
        autosave::write_private_bytes_atomically(&metadata_path, &json)
    }

    fn document_dir(&self, document_key: &str) -> PathBuf {
        self.root.join(digest_hex(document_key.as_bytes()))
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
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn next_version_id(timestamp: u64, digest: &str, versions: &VecDeque<DocumentVersion>) -> String {
    static NEXT_VERSION_ID: AtomicU64 = AtomicU64::new(0);

    loop {
        let sequence = NEXT_VERSION_ID.fetch_add(1, Ordering::Relaxed);
        let id = format!("{timestamp}-{}-{sequence}", short_hash(digest));
        if versions.iter().all(|version| version.id != id) {
            return id;
        }
    }
}

fn validate_versions(versions: &[DocumentVersion]) -> Result<()> {
    anyhow::ensure!(
        versions.len() <= MAX_VERSION_METADATA_ENTRIES,
        "document version metadata contains more than {MAX_VERSION_METADATA_ENTRIES} entries"
    );
    let mut ids = HashSet::with_capacity(versions.len());
    let mut previous_timestamp = None;
    for version in versions {
        anyhow::ensure!(
            !version.id.is_empty() && version.id.len() <= 256,
            "document version has an invalid id"
        );
        anyhow::ensure!(
            ids.insert(version.id.as_str()),
            "duplicate document version id"
        );
        anyhow::ensure!(
            version.digest.len() == 64
                && version
                    .digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "document version has an invalid SHA-256 digest"
        );
        anyhow::ensure!(
            u64::try_from(version.size_bytes).unwrap_or(u64::MAX) <= autosave::MAX_DOCUMENT_BYTES,
            "document version exceeds the document size limit"
        );
        if let Some(previous) = previous_timestamp {
            anyhow::ensure!(
                version.created_at_millis >= previous,
                "document versions are not ordered by creation time"
            );
        }
        previous_timestamp = Some(version.created_at_millis);
    }
    Ok(())
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

    #[test]
    fn repeated_content_is_deduplicated_and_repairs_its_blob() {
        let tmp = TempDir::new().unwrap();
        let store = VersionStore::new_in(tmp.path(), 4).unwrap();
        let first = store.record("doc", b"same").unwrap();
        let blob_path = store
            .document_dir("doc")
            .join(format!("{}.bin", first.digest));
        std::fs::write(&blob_path, b"corrupt").unwrap();

        let second = store.record("doc", b"same").unwrap();
        assert_eq!(first, second);
        assert_eq!(store.load("doc").unwrap().len(), 1);
        assert_eq!(store.read("doc", &second).unwrap(), b"same");
    }

    #[test]
    fn reads_use_trusted_metadata_instead_of_caller_supplied_paths() {
        let tmp = TempDir::new().unwrap();
        let store = VersionStore::new_in(tmp.path(), 4).unwrap();
        let mut forged = store.record("doc", b"trusted").unwrap();
        forged.digest = "../../outside".to_string();
        forged.size_bytes = usize::MAX;

        assert_eq!(store.read("doc", &forged).unwrap(), b"trusted");
    }

    #[test]
    fn rejects_untrusted_version_metadata_before_using_blob_paths() {
        let tmp = TempDir::new().unwrap();
        let store = VersionStore::new_in(tmp.path(), 4).unwrap();
        let document_dir = store.document_dir("doc");
        std::fs::create_dir_all(&document_dir).unwrap();
        let malicious = vec![DocumentVersion {
            id: "malicious".into(),
            created_at_millis: 1,
            digest: "../../outside".into(),
            size_bytes: 1,
        }];
        std::fs::write(
            document_dir.join("versions.json"),
            serde_json::to_vec(&malicious).unwrap(),
        )
        .unwrap();

        assert!(store.load("doc").is_err());
    }

    #[test]
    fn document_directories_use_full_sha256_identities() {
        let tmp = TempDir::new().unwrap();
        let store = VersionStore::new_in(tmp.path(), 4).unwrap();
        let directory = store.document_dir("doc");

        assert_eq!(directory.file_name().unwrap().to_string_lossy().len(), 64);
    }

    #[test]
    fn versions_remain_ordered_when_the_wall_clock_moves_back() {
        let tmp = TempDir::new().unwrap();
        let store = VersionStore::new_in(tmp.path(), 4).unwrap();
        let document_dir = store.document_dir("doc");
        std::fs::create_dir_all(&document_dir).unwrap();
        let existing = DocumentVersion {
            id: "future".into(),
            created_at_millis: u64::MAX,
            digest: digest_hex(b"future"),
            size_bytes: 6,
        };
        autosave::write_private_bytes_atomically(
            &document_dir.join(format!("{}.bin", existing.digest)),
            b"future",
        )
        .unwrap();
        store.persist_versions("doc", &[existing]).unwrap();

        let recorded = store.record("doc", b"new").unwrap();

        assert_eq!(recorded.created_at_millis, u64::MAX);
        assert_eq!(store.load("doc").unwrap().len(), 2);
    }
}
