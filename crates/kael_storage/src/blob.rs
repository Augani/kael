//! Cross-platform byte-oriented persistent storage.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod imp {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };

    use parking_lot::Mutex;
    use rusqlite::{Connection, OptionalExtension as _, params};

    use crate::{Error, Result, platform};

    const MAX_BLOB_BYTES: u64 = 256 * 1024 * 1024;

    /// A durable byte store backed by SQLite on native platforms.
    ///
    /// `BlobStore` has the same asynchronous API on native and browser builds. Browser builds use
    /// IndexedDB, which makes it suitable for document bodies and binary assets that should not be
    /// placed in `localStorage`.
    #[derive(Clone)]
    pub struct BlobStore {
        path: PathBuf,
        connection: Arc<Mutex<Connection>>,
    }

    impl std::fmt::Debug for BlobStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("BlobStore")
                .field("path", &self.path)
                .finish_non_exhaustive()
        }
    }

    impl BlobStore {
        /// Opens the named byte store in the application's platform data directory.
        pub async fn open(app_id: &str, name: &str) -> Result<Self> {
            platform::validate_storage_identifier(app_id)?;
            platform::validate_storage_identifier(name)?;
            let paths = platform::ensure_storage_paths(app_id)?;
            let directory = paths.data_dir.join("blobs");
            std::fs::create_dir_all(&directory)
                .map_err(|source| Error::io(directory.clone(), source))?;
            Self::open_at(directory.join(format!("{name}.sqlite3"))).await
        }

        /// Opens a native byte store at an explicit SQLite file path.
        pub async fn open_at(path: impl AsRef<Path>) -> Result<Self> {
            let path = path.as_ref().to_path_buf();
            let open_path = path.clone();
            let connection = smol::unblock(move || {
                if let Some(parent) = open_path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    std::fs::create_dir_all(parent)
                        .map_err(|source| Error::io(parent.to_path_buf(), source))?;
                }
                let connection = Connection::open(&open_path)?;
                connection.busy_timeout(std::time::Duration::from_secs(5))?;
                connection.execute_batch(
                    "PRAGMA journal_mode = WAL;\nPRAGMA synchronous = NORMAL;\nCREATE TABLE IF NOT EXISTS blobs (\n    key TEXT PRIMARY KEY,\n    value BLOB NOT NULL\n);",
                )?;
                Ok::<_, Error>(connection)
            })
            .await?;
            Ok(Self {
                path,
                connection: Arc::new(Mutex::new(connection)),
            })
        }

        /// Returns the native SQLite path used by this store.
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// Stores or replaces bytes at `key`.
        pub async fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
            validate_key(key)?;
            validate_blob_size(bytes.len())?;
            let connection = self.connection.clone();
            let key = key.to_string();
            let bytes = bytes.to_vec();
            smol::unblock(move || {
                connection.lock().execute(
                    "INSERT INTO blobs (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, bytes],
                )?;
                Ok(())
            })
            .await
        }

        /// Stores several values in one SQLite transaction.
        ///
        /// Either every value commits or no value is changed. Duplicate keys are applied in input
        /// order, so the last value wins.
        pub async fn put_many(&self, entries: &[(&str, &[u8])]) -> Result<()> {
            let entries = entries
                .iter()
                .map(|(key, bytes)| {
                    validate_key(key)?;
                    validate_blob_size(bytes.len())?;
                    Ok(((*key).to_string(), (*bytes).to_vec()))
                })
                .collect::<Result<Vec<_>>>()?;
            if entries.is_empty() {
                return Ok(());
            }
            let connection = self.connection.clone();
            smol::unblock(move || {
                let mut connection = connection.lock();
                let transaction = connection.transaction()?;
                {
                    let mut statement = transaction.prepare(
                        "INSERT INTO blobs (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    )?;
                    for (key, bytes) in entries {
                        statement.execute(params![key, bytes])?;
                    }
                }
                transaction.commit()?;
                Ok(())
            })
            .await
        }

        /// Loads bytes stored at `key`.
        pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
            validate_key(key)?;
            let connection = self.connection.clone();
            let key = key.to_string();
            smol::unblock(move || {
                let value = connection
                    .lock()
                    .query_row(
                        "SELECT value FROM blobs WHERE key = ?1",
                        params![key],
                        |row| row.get::<_, Vec<u8>>(0),
                    )
                    .optional()?;
                if let Some(bytes) = &value {
                    validate_blob_size(bytes.len())?;
                }
                Ok(value)
            })
            .await
        }

        /// Returns whether `key` exists.
        pub async fn contains_key(&self, key: &str) -> Result<bool> {
            validate_key(key)?;
            let connection = self.connection.clone();
            let key = key.to_string();
            smol::unblock(move || {
                Ok(connection
                    .lock()
                    .query_row("SELECT 1 FROM blobs WHERE key = ?1", params![key], |_| {
                        Ok(())
                    })
                    .optional()?
                    .is_some())
            })
            .await
        }

        /// Removes `key`, returning whether it existed.
        pub async fn remove(&self, key: &str) -> Result<bool> {
            validate_key(key)?;
            let connection = self.connection.clone();
            let key = key.to_string();
            smol::unblock(move || {
                Ok(connection
                    .lock()
                    .execute("DELETE FROM blobs WHERE key = ?1", params![key])?
                    > 0)
            })
            .await
        }

        /// Removes several keys in one SQLite transaction and returns the number that existed.
        pub async fn remove_many(&self, keys: &[&str]) -> Result<usize> {
            let keys = keys
                .iter()
                .map(|key| {
                    validate_key(key)?;
                    Ok((*key).to_string())
                })
                .collect::<Result<Vec<_>>>()?;
            if keys.is_empty() {
                return Ok(0);
            }
            let connection = self.connection.clone();
            smol::unblock(move || {
                let mut connection = connection.lock();
                let transaction = connection.transaction()?;
                let mut removed = 0usize;
                {
                    let mut statement = transaction.prepare("DELETE FROM blobs WHERE key = ?1")?;
                    for key in keys {
                        removed = removed.saturating_add(statement.execute(params![key])?);
                    }
                }
                transaction.commit()?;
                Ok(removed)
            })
            .await
        }

        /// Returns every key in deterministic order.
        pub async fn keys(&self) -> Result<Vec<String>> {
            let connection = self.connection.clone();
            smol::unblock(move || {
                let connection = connection.lock();
                let mut statement = connection.prepare("SELECT key FROM blobs ORDER BY key")?;
                let keys = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                for key in &keys {
                    validate_key(key)?;
                }
                Ok(keys)
            })
            .await
        }

        /// Removes all values and returns the number removed.
        pub async fn clear(&self) -> Result<usize> {
            let connection = self.connection.clone();
            smol::unblock(move || Ok(connection.lock().execute("DELETE FROM blobs", [])?)).await
        }
    }

    fn validate_key(key: &str) -> Result<()> {
        if key.is_empty() || key.len() > 4 * 1024 || key.chars().any(char::is_control) {
            Err(Error::InvalidStorageKey)
        } else {
            Ok(())
        }
    }

    fn validate_blob_size(size: usize) -> Result<()> {
        let actual = u64::try_from(size).unwrap_or(u64::MAX);
        if actual > MAX_BLOB_BYTES {
            Err(Error::BlobTooLarge {
                actual,
                limit: MAX_BLOB_BYTES,
            })
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::BlobStore;

        #[test]
        fn round_trips_binary_values() {
            let directory = tempfile::tempdir().unwrap();
            let store = futures::executor::block_on(BlobStore::open_at(
                directory.path().join("blobs.sqlite3"),
            ))
            .unwrap();
            futures::executor::block_on(store.put("document", &[0, 1, 2, 255])).unwrap();
            assert_eq!(
                futures::executor::block_on(store.get("document")).unwrap(),
                Some(vec![0, 1, 2, 255])
            );
            assert!(futures::executor::block_on(store.contains_key("document")).unwrap());
            assert_eq!(
                futures::executor::block_on(store.keys()).unwrap(),
                vec!["document"]
            );
            assert!(futures::executor::block_on(store.remove("document")).unwrap());

            futures::executor::block_on(store.put_many(&[("first", b"one"), ("second", b"two")]))
                .unwrap();
            assert_eq!(
                futures::executor::block_on(store.remove_many(&["first", "second"])).unwrap(),
                2
            );
        }
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod imp {
    use std::rc::Rc;

    use js_sys::Uint8Array;
    use rexie::{ObjectStore, Rexie, TransactionMode};
    use wasm_bindgen::{JsCast as _, JsValue};

    use crate::{Error, Result, platform};

    const STORE_NAME: &str = "blobs";
    const MAX_BLOB_BYTES: u64 = 256 * 1024 * 1024;

    /// A durable byte store backed by browser IndexedDB.
    ///
    /// Values are stored as `Uint8Array` records rather than encoded strings. Every mutation waits
    /// for its IndexedDB transaction to commit before returning success.
    #[derive(Clone)]
    pub struct BlobStore {
        database_name: String,
        database: Rc<Rexie>,
    }

    impl std::fmt::Debug for BlobStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("BlobStore")
                .field("database_name", &self.database_name)
                .finish_non_exhaustive()
        }
    }

    impl BlobStore {
        /// Opens the named IndexedDB byte store for an application.
        pub async fn open(app_id: &str, name: &str) -> Result<Self> {
            platform::validate_storage_identifier(app_id)?;
            platform::validate_storage_identifier(name)?;
            let database_name = format!("kael:blob:v1:{app_id}:{name}");
            let database = Rexie::builder(&database_name)
                .version(1)
                .add_object_store(ObjectStore::new(STORE_NAME))
                .build()
                .await
                .map_err(|error| indexed_db_error("open", &error))?;
            Ok(Self {
                database_name,
                database: Rc::new(database),
            })
        }

        /// Returns the origin-scoped IndexedDB database name.
        pub fn database_name(&self) -> &str {
            &self.database_name
        }

        /// Stores or replaces bytes at `key` and waits for the transaction to commit.
        pub async fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
            validate_key(key)?;
            validate_blob_size(bytes.len())?;
            let transaction = self
                .database
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|error| indexed_db_error("put.transaction", &error))?;
            let store = transaction
                .store(STORE_NAME)
                .map_err(|error| indexed_db_error("put.store", &error))?;
            let value = Uint8Array::from(bytes);
            let key = JsValue::from_str(key);
            store
                .put(value.as_ref(), Some(&key))
                .await
                .map_err(|error| indexed_db_error("put", &error))?;
            transaction
                .done()
                .await
                .map_err(|error| indexed_db_error("put.commit", &error))?;
            Ok(())
        }

        /// Stores several values in one IndexedDB transaction.
        ///
        /// Either every value commits or the transaction aborts. Duplicate keys are applied in
        /// input order, so the last value wins.
        pub async fn put_many(&self, entries: &[(&str, &[u8])]) -> Result<()> {
            for (key, bytes) in entries {
                validate_key(key)?;
                validate_blob_size(bytes.len())?;
            }
            if entries.is_empty() {
                return Ok(());
            }
            let transaction = self
                .database
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|error| indexed_db_error("put_many.transaction", &error))?;
            let store = transaction
                .store(STORE_NAME)
                .map_err(|error| indexed_db_error("put_many.store", &error))?;
            for (key, bytes) in entries {
                let value = Uint8Array::from(*bytes);
                let key = JsValue::from_str(key);
                store
                    .put(value.as_ref(), Some(&key))
                    .await
                    .map_err(|error| indexed_db_error("put_many", &error))?;
            }
            transaction
                .done()
                .await
                .map_err(|error| indexed_db_error("put_many.commit", &error))?;
            Ok(())
        }

        /// Loads bytes stored at `key`.
        pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
            validate_key(key)?;
            let transaction = self
                .database
                .transaction(&[STORE_NAME], TransactionMode::ReadOnly)
                .map_err(|error| indexed_db_error("get.transaction", &error))?;
            let store = transaction
                .store(STORE_NAME)
                .map_err(|error| indexed_db_error("get.store", &error))?;
            let value = store
                .get(JsValue::from_str(key))
                .await
                .map_err(|error| indexed_db_error("get", &error))?;
            transaction
                .done()
                .await
                .map_err(|error| indexed_db_error("get.complete", &error))?;
            value
                .map(|value| {
                    if !value.is_instance_of::<Uint8Array>() {
                        return Err(Error::BrowserStorage {
                            operation: "get",
                            message: "IndexedDB record was not a Uint8Array".to_string(),
                        });
                    }
                    let bytes = Uint8Array::new(&value).to_vec();
                    validate_blob_size(bytes.len())?;
                    Ok(bytes)
                })
                .transpose()
        }

        /// Returns whether `key` exists.
        pub async fn contains_key(&self, key: &str) -> Result<bool> {
            validate_key(key)?;
            let transaction = self
                .database
                .transaction(&[STORE_NAME], TransactionMode::ReadOnly)
                .map_err(|error| indexed_db_error("contains_key.transaction", &error))?;
            let store = transaction
                .store(STORE_NAME)
                .map_err(|error| indexed_db_error("contains_key.store", &error))?;
            let exists = store
                .key_exists(JsValue::from_str(key))
                .await
                .map_err(|error| indexed_db_error("contains_key", &error))?;
            transaction
                .done()
                .await
                .map_err(|error| indexed_db_error("contains_key.complete", &error))?;
            Ok(exists)
        }

        /// Removes `key`, returning whether it existed, and waits for commit.
        pub async fn remove(&self, key: &str) -> Result<bool> {
            validate_key(key)?;
            let transaction = self
                .database
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|error| indexed_db_error("remove.transaction", &error))?;
            let store = transaction
                .store(STORE_NAME)
                .map_err(|error| indexed_db_error("remove.store", &error))?;
            let key = JsValue::from_str(key);
            let existed = store
                .key_exists(key.clone())
                .await
                .map_err(|error| indexed_db_error("remove.exists", &error))?;
            if existed {
                store
                    .delete(key)
                    .await
                    .map_err(|error| indexed_db_error("remove", &error))?;
            }
            transaction
                .done()
                .await
                .map_err(|error| indexed_db_error("remove.commit", &error))?;
            Ok(existed)
        }

        /// Removes several keys in one IndexedDB transaction and returns the number that existed.
        pub async fn remove_many(&self, keys: &[&str]) -> Result<usize> {
            for key in keys {
                validate_key(key)?;
            }
            if keys.is_empty() {
                return Ok(0);
            }
            let transaction = self
                .database
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|error| indexed_db_error("remove_many.transaction", &error))?;
            let store = transaction
                .store(STORE_NAME)
                .map_err(|error| indexed_db_error("remove_many.store", &error))?;
            let mut removed = 0usize;
            for key in keys {
                let key = JsValue::from_str(key);
                if store
                    .key_exists(key.clone())
                    .await
                    .map_err(|error| indexed_db_error("remove_many.exists", &error))?
                {
                    store
                        .delete(key)
                        .await
                        .map_err(|error| indexed_db_error("remove_many", &error))?;
                    removed = removed.saturating_add(1);
                }
            }
            transaction
                .done()
                .await
                .map_err(|error| indexed_db_error("remove_many.commit", &error))?;
            Ok(removed)
        }

        /// Returns every key in deterministic order.
        pub async fn keys(&self) -> Result<Vec<String>> {
            let transaction = self
                .database
                .transaction(&[STORE_NAME], TransactionMode::ReadOnly)
                .map_err(|error| indexed_db_error("keys.transaction", &error))?;
            let store = transaction
                .store(STORE_NAME)
                .map_err(|error| indexed_db_error("keys.store", &error))?;
            let values = store
                .get_all_keys(None, None)
                .await
                .map_err(|error| indexed_db_error("keys", &error))?;
            transaction
                .done()
                .await
                .map_err(|error| indexed_db_error("keys.complete", &error))?;
            let mut keys = values
                .into_iter()
                .map(|value| {
                    value.as_string().ok_or_else(|| Error::BrowserStorage {
                        operation: "keys",
                        message: "IndexedDB key was not a string".to_string(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            for key in &keys {
                validate_key(key)?;
            }
            keys.sort();
            Ok(keys)
        }

        /// Removes all values and returns the number removed.
        pub async fn clear(&self) -> Result<usize> {
            let transaction = self
                .database
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|error| indexed_db_error("clear.transaction", &error))?;
            let store = transaction
                .store(STORE_NAME)
                .map_err(|error| indexed_db_error("clear.store", &error))?;
            let count = store
                .count(None)
                .await
                .map_err(|error| indexed_db_error("clear.count", &error))?;
            store
                .clear()
                .await
                .map_err(|error| indexed_db_error("clear", &error))?;
            transaction
                .done()
                .await
                .map_err(|error| indexed_db_error("clear.commit", &error))?;
            Ok(usize::try_from(count).unwrap_or(usize::MAX))
        }
    }

    fn validate_key(key: &str) -> Result<()> {
        if key.is_empty() || key.len() > 4 * 1024 || key.chars().any(char::is_control) {
            Err(Error::InvalidStorageKey)
        } else {
            Ok(())
        }
    }

    fn validate_blob_size(size: usize) -> Result<()> {
        let actual = u64::try_from(size).unwrap_or(u64::MAX);
        if actual > MAX_BLOB_BYTES {
            Err(Error::BlobTooLarge {
                actual,
                limit: MAX_BLOB_BYTES,
            })
        } else {
            Ok(())
        }
    }

    fn indexed_db_error(operation: &'static str, error: &rexie::Error) -> Error {
        Error::BrowserStorage {
            operation,
            message: format!("{error}: {error:?}"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::BlobStore;
        use wasm_bindgen_test::wasm_bindgen_test;

        #[wasm_bindgen_test]
        async fn indexed_db_round_trips_binary_values_and_commits() {
            let app_id = format!("kael.blob.test.{}", js_sys::Date::now());
            let store = BlobStore::open(&app_id, "documents").await.unwrap();
            assert_eq!(store.clear().await.unwrap(), 0);
            store.put("binary", &[0, 1, 2, 255]).await.unwrap();
            assert_eq!(store.get("binary").await.unwrap(), Some(vec![0, 1, 2, 255]));
            assert!(store.contains_key("binary").await.unwrap());
            assert_eq!(store.keys().await.unwrap(), vec!["binary"]);

            let reopened = BlobStore::open(&app_id, "documents").await.unwrap();
            assert_eq!(
                reopened.get("binary").await.unwrap(),
                Some(vec![0, 1, 2, 255])
            );
            assert!(reopened.remove("binary").await.unwrap());
            reopened
                .put_many(&[("first", b"one"), ("second", b"two")])
                .await
                .unwrap();
            assert_eq!(reopened.remove_many(&["first", "second"]).await.unwrap(), 2);
        }
    }
}

pub use imp::BlobStore;
