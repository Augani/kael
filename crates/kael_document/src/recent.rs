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
    #[serde(with = "path_serde")]
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
        autosave::ensure_real_directory(root, "recent document root")?;
        Ok(Self {
            path: root.join("recent_documents.json"),
            max_entries: max_entries.max(1),
        })
    }

    pub(crate) fn load(&self) -> Result<Vec<RecentDocument>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let json = autosave::read_regular_file_bounded(
            &self.path,
            MAX_RECENT_METADATA_BYTES,
            "recent document metadata",
        )?;
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
        let next_timestamp = documents.first().map_or_else(now_unix_millis, |latest| {
            now_unix_millis().max(latest.last_opened_at_millis.saturating_add(1))
        });
        documents.insert(
            0,
            RecentDocument {
                path: path.to_path_buf(),
                last_opened_at_millis: next_timestamp,
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
    autosave::write_private_bytes_atomically(path, &json)
}

fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

mod path_serde {
    use std::path::PathBuf;

    use serde::{Deserialize as _, Deserializer, Serialize as _, Serializer, de::Error as _};

    pub(super) fn serialize<S>(path: &PathBuf, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(path) = path.to_str() {
            return serializer.serialize_str(path);
        }

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;

            let mut object = serde_json::Map::new();
            object.insert(
                "unix_bytes".to_string(),
                serde_json::Value::String(hex_encode(path.as_os_str().as_bytes())),
            );
            return serde_json::Value::Object(object).serialize(serializer);
        }

        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt as _;

            let mut object = serde_json::Map::new();
            object.insert(
                "windows_wide".to_string(),
                serde_json::Value::Array(
                    path.as_os_str()
                        .encode_wide()
                        .map(|unit| serde_json::Value::from(u64::from(unit)))
                        .collect(),
                ),
            );
            return serde_json::Value::Object(object).serialize(serializer);
        }

        #[cfg(not(any(unix, windows)))]
        serializer.serialize_str(&path.to_string_lossy())
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(path) => Ok(PathBuf::from(path)),
            serde_json::Value::Object(mut object) => {
                #[cfg(unix)]
                if object.len() == 1
                    && let Some(serde_json::Value::String(bytes)) = object.remove("unix_bytes")
                {
                    use std::os::unix::ffi::OsStringExt as _;

                    return Ok(PathBuf::from(std::ffi::OsString::from_vec(
                        hex_decode(&bytes).map_err(D::Error::custom)?,
                    )));
                }

                #[cfg(windows)]
                if object.len() == 1
                    && let Some(serde_json::Value::Array(units)) = object.remove("windows_wide")
                {
                    use std::os::windows::ffi::OsStringExt as _;

                    let units = units
                        .into_iter()
                        .map(|unit| {
                            let unit = unit.as_u64().ok_or_else(|| {
                                D::Error::custom("Windows path unit must be an integer")
                            })?;
                            u16::try_from(unit)
                                .map_err(|_| D::Error::custom("Windows path unit exceeds u16"))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(PathBuf::from(std::ffi::OsString::from_wide(&units)));
                }

                Err(D::Error::custom(
                    "recent document path uses unsupported native encoding",
                ))
            }
            _ => Err(D::Error::custom(
                "recent document path must be a string or native path object",
            )),
        }
    }

    #[cfg(unix)]
    fn hex_encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
        for byte in bytes {
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        encoded
    }

    #[cfg(unix)]
    fn hex_decode(encoded: &str) -> Result<Vec<u8>, &'static str> {
        if !encoded.len().is_multiple_of(2) {
            return Err("Unix path byte encoding has an odd length");
        }
        let mut decoded = Vec::new();
        decoded
            .try_reserve_exact(encoded.len() / 2)
            .map_err(|_| "failed to reserve Unix path bytes")?;
        for pair in encoded.as_bytes().chunks_exact(2) {
            let high = hex_value(pair[0]).ok_or("Unix path contains invalid hexadecimal")?;
            let low = hex_value(pair[1]).ok_or("Unix path contains invalid hexadecimal")?;
            decoded.push((high << 4) | low);
        }
        Ok(decoded)
    }

    #[cfg(unix)]
    fn hex_value(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
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

    #[test]
    fn recording_stays_newest_when_the_wall_clock_moves_back() {
        let directory = tempfile::tempdir().unwrap();
        let store = RecentDocumentStore::new_in(directory.path(), 2).unwrap();
        let existing = vec![RecentDocument {
            path: PathBuf::from("future"),
            last_opened_at_millis: u64::MAX - 1,
        }];
        persist_recent_documents(&store.path, &existing).unwrap();

        store.record(Path::new("new")).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded[0].path, Path::new("new"));
        assert_eq!(loaded[0].last_opened_at_millis, u64::MAX);
    }

    #[cfg(unix)]
    #[test]
    fn native_non_utf8_paths_round_trip() {
        use std::os::unix::ffi::OsStringExt as _;

        let directory = tempfile::tempdir().unwrap();
        let store = RecentDocumentStore::new_in(directory.path(), 2).unwrap();
        let path = PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'n', b'o', b'n', 0xff, b'u', b't', b'f', b'8',
        ]));

        store.record(&path).unwrap();

        assert_eq!(store.load().unwrap()[0].path, path);
    }
}
