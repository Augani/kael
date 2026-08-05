//! Typed key-value storage backends.

use std::{
    collections::{BTreeMap, HashMap},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tempfile::NamedTempFile;

use crate::{Error, Result, Subscription, platform};

type Observer = Arc<dyn Fn(Option<Value>) + Send + Sync + 'static>;

const MAX_JSON_STORE_BYTES: u64 = 64 * 1024 * 1024;

struct KvState {
    values: BTreeMap<String, Value>,
    observers: HashMap<String, BTreeMap<usize, Observer>>,
    next_observer_id: usize,
}

impl KvState {
    fn allocate_observer_id(&mut self) -> usize {
        allocate_observer_id(&mut self.next_observer_id, &self.observers)
    }
}

/// Common behavior for typed key-value stores.
pub trait KvStore: Send + Sync {
    /// Returns the deserialized value stored at `key`.
    fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>>;

    /// Serializes and persists a value at `key`.
    fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<()>;

    /// Removes a value and returns whether the key existed.
    fn remove(&self, key: &str) -> Result<bool>;

    /// Returns all known keys in deterministic order.
    fn keys(&self) -> Result<Vec<String>>;

    /// Observes changes to a key and immediately reports its current value.
    ///
    /// Registration fails if the backend cannot read the current value. Later
    /// type mismatches are delivered to `callback` instead of being reported as
    /// a missing value.
    fn observe<T, F>(&self, key: &str, callback: F) -> Result<Subscription>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(Result<Option<T>>) + Send + Sync + 'static;
}

/// A JSON-file-backed key-value store.
#[derive(Clone)]
pub struct JsonKvStore {
    path: PathBuf,
    state: Arc<Mutex<KvState>>,
}

impl std::fmt::Debug for JsonKvStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonKvStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

struct SqliteObserverState {
    observers: HashMap<String, BTreeMap<usize, Observer>>,
    next_observer_id: usize,
}

impl SqliteObserverState {
    fn allocate_observer_id(&mut self) -> usize {
        allocate_observer_id(&mut self.next_observer_id, &self.observers)
    }
}

/// A SQLite-backed key-value store.
#[derive(Clone)]
pub struct SqliteKvStore {
    path: PathBuf,
    connection: Arc<Mutex<Connection>>,
    observers: Arc<Mutex<SqliteObserverState>>,
}

impl std::fmt::Debug for SqliteKvStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteKvStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// The default SQLite-backed key-value store implementation for the current platform.
pub type PlatformKvStore = SqliteKvStore;

impl JsonKvStore {
    /// Opens the default key-value store for the given application identifier.
    pub fn open(app_id: &str) -> Result<Self> {
        let paths = platform::ensure_storage_paths(app_id)?;
        Self::open_at(paths.preferences_path)
    }

    /// Opens a key-value store at an explicit JSON file path.
    pub fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|source| Error::io(parent.to_path_buf(), source))?;
        }

        let values = load_values(&path)?;

        Ok(Self {
            path,
            state: Arc::new(Mutex::new(KvState {
                values,
                observers: HashMap::new(),
                next_observer_id: 0,
            })),
        })
    }

    /// Returns the JSON file used by this store.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn notify(observers: Vec<Observer>, value: Option<Value>) {
        for observer in observers {
            observer(value.clone());
        }
    }
}

impl SqliteKvStore {
    /// Opens the default key-value store for the given application identifier.
    pub fn open(app_id: &str) -> Result<Self> {
        let paths = platform::ensure_storage_paths(app_id)?;
        let path = sqlite_preferences_path(&paths.preferences_path);
        let store = Self::open_at(&path)?;
        store.import_legacy_json_if_empty(&paths.preferences_path)?;
        Ok(store)
    }

    /// Opens a key-value store at an explicit SQLite file path.
    pub fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|source| Error::io(parent.to_path_buf(), source))?;
        }

        let connection = Connection::open(&path)?;
        configure_sqlite_kv_connection(&connection)?;

        Ok(Self {
            path,
            connection: Arc::new(Mutex::new(connection)),
            observers: Arc::new(Mutex::new(SqliteObserverState {
                observers: HashMap::new(),
                next_observer_id: 0,
            })),
        })
    }

    /// Returns the SQLite file used by this store.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn import_legacy_json_if_empty(&self, legacy_json_path: &Path) -> Result<()> {
        if !legacy_json_path.exists() {
            return Ok(());
        }

        let mut connection = self.connection.lock();
        let existing_rows: i64 =
            connection.query_row("SELECT COUNT(*) FROM kv_entries", [], |row| row.get(0))?;
        if existing_rows > 0 {
            return Ok(());
        }

        let values = load_values(legacy_json_path)?;
        if values.is_empty() {
            return Ok(());
        }

        let transaction = connection.transaction()?;
        {
            let mut statement =
                transaction.prepare("INSERT INTO kv_entries (key, value) VALUES (?1, ?2)")?;
            for (key, value) in values {
                let json = serde_json::to_string(&value)?;
                statement.execute(params![key, json])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

impl KvStore for JsonKvStore {
    fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let state = self.state.lock();
        let Some(value) = state.values.get(key).cloned() else {
            return Ok(None);
        };

        serde_json::from_value(value)
            .map(Some)
            .map_err(|source| Error::DeserializeValue {
                key: key.to_string(),
                source,
            })
    }

    fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let serialized = serde_json::to_value(value).map_err(|source| Error::SerializeValue {
            key: key.to_string(),
            source,
        })?;

        let observers = {
            let mut state = self.state.lock();

            if state.values.get(key) == Some(&serialized) {
                return Ok(());
            }

            let previous = state.values.insert(key.to_string(), serialized.clone());
            if let Err(error) = persist_values(&self.path, &state.values) {
                match previous {
                    Some(previous) => {
                        state.values.insert(key.to_string(), previous);
                    }
                    None => {
                        state.values.remove(key);
                    }
                }
                return Err(error);
            }

            state
                .observers
                .get(key)
                .map(|obs| obs.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        };

        Self::notify(observers, Some(serialized));
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<bool> {
        let observers = {
            let mut state = self.state.lock();

            if !state.values.contains_key(key) {
                return Ok(false);
            }

            let Some(previous) = state.values.remove(key) else {
                return Ok(false);
            };
            if let Err(error) = persist_values(&self.path, &state.values) {
                state.values.insert(key.to_string(), previous);
                return Err(error);
            }

            state
                .observers
                .get(key)
                .map(|obs| obs.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        };

        Self::notify(observers, None);
        Ok(true)
    }

    fn keys(&self) -> Result<Vec<String>> {
        Ok(self.state.lock().values.keys().cloned().collect())
    }

    fn observe<T, F>(&self, key: &str, callback: F) -> Result<Subscription>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(Result<Option<T>>) + Send + Sync + 'static,
    {
        let observer = typed_observer(key, callback);

        let key = key.to_string();
        let observer_id;
        let current_value = {
            let mut state = self.state.lock();
            observer_id = state.allocate_observer_id();
            state
                .observers
                .entry(key.clone())
                .or_default()
                .insert(observer_id, observer.clone());
            state.values.get(&key).cloned()
        };

        observer(current_value);

        let state = self.state.clone();
        let unsubscribe_key = key;
        Ok(Subscription::new(move || {
            let mut state = state.lock();
            if let Some(observers) = state.observers.get_mut(&unsubscribe_key) {
                observers.remove(&observer_id);
                if observers.is_empty() {
                    state.observers.remove(&unsubscribe_key);
                }
            }
        }))
    }
}

impl KvStore for SqliteKvStore {
    fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let connection = self.connection.lock();
        let Some(value) = load_value_from_connection(&connection, key)? else {
            return Ok(None);
        };

        serde_json::from_value(value)
            .map(Some)
            .map_err(|source| Error::DeserializeValue {
                key: key.to_string(),
                source,
            })
    }

    fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let serialized = serde_json::to_value(value).map_err(|source| Error::SerializeValue {
            key: key.to_string(),
            source,
        })?;
        let json = serde_json::to_string(&serialized)?;

        {
            let connection = self.connection.lock();
            if load_value_from_connection(&connection, key)?.as_ref() == Some(&serialized) {
                return Ok(());
            }

            connection.execute(
                "INSERT INTO kv_entries (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, json],
            )?;
        }

        let observers = {
            let observers = self.observers.lock();
            observers_for_key(&observers.observers, key)
        };
        JsonKvStore::notify(observers, Some(serialized));
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<bool> {
        let removed = {
            let connection = self.connection.lock();
            connection.execute("DELETE FROM kv_entries WHERE key = ?1", params![key])? > 0
        };

        if !removed {
            return Ok(false);
        }

        let observers = {
            let observers = self.observers.lock();
            observers_for_key(&observers.observers, key)
        };
        JsonKvStore::notify(observers, None);
        Ok(true)
    }

    fn keys(&self) -> Result<Vec<String>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare("SELECT key FROM kv_entries ORDER BY key")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    fn observe<T, F>(&self, key: &str, callback: F) -> Result<Subscription>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(Result<Option<T>>) + Send + Sync + 'static,
    {
        let observer = typed_observer(key, callback);

        let key = key.to_string();
        let (observer_id, current_value) = {
            let connection = self.connection.lock();
            let current_value = load_value_from_connection(&connection, &key)?;
            let mut state = self.observers.lock();
            let observer_id = state.allocate_observer_id();
            state
                .observers
                .entry(key.clone())
                .or_default()
                .insert(observer_id, observer.clone());
            (observer_id, current_value)
        };

        observer(current_value);

        let observers = self.observers.clone();
        let unsubscribe_key = key;
        Ok(Subscription::new(move || {
            let mut state = observers.lock();
            if let Some(observers) = state.observers.get_mut(&unsubscribe_key) {
                observers.remove(&observer_id);
                if observers.is_empty() {
                    state.observers.remove(&unsubscribe_key);
                }
            }
        }))
    }
}

fn configure_sqlite_kv_connection(connection: &Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;\nPRAGMA synchronous = NORMAL;\nCREATE TABLE IF NOT EXISTS kv_entries (\n    key TEXT PRIMARY KEY,\n    value TEXT NOT NULL\n);",
    )?;
    Ok(())
}

fn load_value_from_connection(connection: &Connection, key: &str) -> Result<Option<Value>> {
    let stored = connection
        .query_row(
            "SELECT value FROM kv_entries WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    stored
        .map(|json| {
            serde_json::from_str(&json).map_err(|source| Error::DeserializeValue {
                key: key.to_string(),
                source,
            })
        })
        .transpose()
}

fn sqlite_preferences_path(preferences_path: &Path) -> PathBuf {
    preferences_path.with_extension("sqlite3")
}

fn observers_for_key(
    observers: &HashMap<String, BTreeMap<usize, Observer>>,
    key: &str,
) -> Vec<Observer> {
    observers
        .get(key)
        .map(|observers| observers.values().cloned().collect())
        .unwrap_or_default()
}

fn typed_observer<T, F>(key: &str, callback: F) -> Observer
where
    T: DeserializeOwned + Send + 'static,
    F: Fn(Result<Option<T>>) + Send + Sync + 'static,
{
    let key = key.to_string();
    Arc::new(move |value| {
        let value = value
            .map(|value| {
                serde_json::from_value(value).map_err(|source| Error::DeserializeValue {
                    key: key.clone(),
                    source,
                })
            })
            .transpose();
        callback(value);
    })
}

fn allocate_observer_id(
    next_observer_id: &mut usize,
    observers: &HashMap<String, BTreeMap<usize, Observer>>,
) -> usize {
    loop {
        let candidate = *next_observer_id;
        *next_observer_id = next_observer_id.checked_add(1).unwrap_or(0);
        if observers
            .values()
            .all(|observers| !observers.contains_key(&candidate))
        {
            return candidate;
        }
    }
}

fn load_values(path: &Path) -> Result<BTreeMap<String, Value>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let metadata =
        std::fs::metadata(path).map_err(|source| Error::io(path.to_path_buf(), source))?;
    ensure_json_store_size(metadata.len())?;
    let contents = std::fs::read(path).map_err(|source| Error::io(path.to_path_buf(), source))?;
    ensure_json_store_size(contents.len().try_into().unwrap_or(u64::MAX))?;
    Ok(serde_json::from_slice(&contents)?)
}

fn ensure_json_store_size(actual: u64) -> Result<()> {
    if actual > MAX_JSON_STORE_BYTES {
        Err(Error::JsonStoreTooLarge {
            actual,
            limit: MAX_JSON_STORE_BYTES,
        })
    } else {
        Ok(())
    }
}

fn persist_values(path: &Path, values: &BTreeMap<String, Value>) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|source| Error::io(parent.to_path_buf(), source))?;
    }

    let contents = serde_json::to_vec(values)?;
    ensure_json_store_size(contents.len().try_into().unwrap_or(u64::MAX))?;
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file =
        NamedTempFile::new_in(directory).map_err(|source| Error::io(path.to_path_buf(), source))?;
    file.write_all(&contents)
        .map_err(|source| Error::io(path.to_path_buf(), source))?;
    file.flush()
        .map_err(|source| Error::io(path.to_path_buf(), source))?;
    file.as_file()
        .sync_all()
        .map_err(|source| Error::io(path.to_path_buf(), source))?;
    file.persist(path)
        .map_err(|error| Error::io(path.to_path_buf(), error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    use rusqlite::params;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    use super::{
        Error, JsonKvStore, KvStore, MAX_JSON_STORE_BYTES, SqliteKvStore, ensure_json_store_size,
        persist_values,
    };

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Preferences {
        theme: String,
        recent_ids: Vec<u64>,
    }

    #[test]
    fn round_trips_complex_values() {
        let directory = tempdir().unwrap();
        let store = JsonKvStore::open_at(directory.path().join("preferences.json")).unwrap();
        let preferences = Preferences {
            theme: "dark".to_string(),
            recent_ids: vec![1, 2, 3],
        };

        store.set("preferences", &preferences).unwrap();
        let restored = store.get::<Preferences>("preferences").unwrap();

        assert_eq!(restored, Some(preferences));
        assert_eq!(store.keys().unwrap(), vec!["preferences".to_string()]);
    }

    #[test]
    fn notifies_observers_on_registration_and_change() {
        let directory = tempdir().unwrap();
        let store = JsonKvStore::open_at(directory.path().join("preferences.json")).unwrap();
        let values = Arc::new(Mutex::new(Vec::new()));
        let observed = values.clone();

        let _subscription = store
            .observe::<String, _>("theme", move |value| {
                observed.lock().unwrap().push(value.unwrap());
            })
            .unwrap();

        store.set("theme", &"dark").unwrap();
        store.remove("theme").unwrap();

        let observed = values.lock().unwrap().clone();
        assert_eq!(observed, vec![None, Some("dark".to_string()), None]);
    }

    #[test]
    fn sqlite_store_round_trips_complex_values() {
        let directory = tempdir().unwrap();
        let store = SqliteKvStore::open_at(directory.path().join("preferences.sqlite3")).unwrap();
        let preferences = Preferences {
            theme: "dark".to_string(),
            recent_ids: vec![1, 2, 3],
        };

        store.set("preferences", &preferences).unwrap();
        let restored = store.get::<Preferences>("preferences").unwrap();

        assert_eq!(restored, Some(preferences));
        assert_eq!(store.keys().unwrap(), vec!["preferences".to_string()]);
    }

    #[test]
    fn sqlite_store_notifies_observers_on_registration_and_change() {
        let directory = tempdir().unwrap();
        let store = SqliteKvStore::open_at(directory.path().join("preferences.sqlite3")).unwrap();
        let values = Arc::new(Mutex::new(Vec::new()));
        let observed = values.clone();

        let _subscription = store
            .observe::<String, _>("theme", move |value| {
                observed.lock().unwrap().push(value.unwrap());
            })
            .unwrap();

        store.set("theme", &"dark").unwrap();
        store.remove("theme").unwrap();

        let observed = values.lock().unwrap().clone();
        assert_eq!(observed, vec![None, Some("dark".to_string()), None]);
    }

    #[test]
    fn sqlite_observer_sees_initial_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteKvStore::open_at(dir.path().join("test.db")).unwrap();
        store.set("color", &"blue".to_string()).unwrap();

        let received = Arc::new(Mutex::new(Vec::new()));
        let recv_clone = received.clone();
        let _sub = store
            .observe::<String, _>("color", move |value| {
                recv_clone.lock().unwrap().push(value.unwrap());
            })
            .unwrap();

        let values = received.lock().unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], Some("blue".to_string()));
    }

    #[test]
    fn json_concurrent_sets_do_not_lose_writes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(JsonKvStore::open_at(dir.path().join("test.json")).unwrap());

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let store = store.clone();
                std::thread::spawn(move || {
                    for j in 0..20 {
                        let key = format!("key-{i}-{j}");
                        store.set(&key, &format!("val-{i}-{j}")).unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        for i in 0..8 {
            for j in 0..20 {
                let key = format!("key-{i}-{j}");
                let val: Option<String> = store.get(&key).unwrap();
                assert!(val.is_some(), "missing {key}");
            }
        }
    }

    #[test]
    fn json_store_atomically_replaces_an_existing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        let store = JsonKvStore::open_at(&path).unwrap();

        store.set("theme", &"light").unwrap();
        store.set("theme", &"dark").unwrap();

        let reopened = JsonKvStore::open_at(path).unwrap();
        assert_eq!(
            reopened.get::<String>("theme").unwrap().as_deref(),
            Some("dark")
        );
    }

    #[test]
    fn observer_reports_type_mismatches() {
        let directory = tempdir().unwrap();
        let store = JsonKvStore::open_at(directory.path().join("preferences.json")).unwrap();
        let results = Arc::new(Mutex::new(Vec::new()));
        let observed = results.clone();

        let _subscription = store
            .observe::<String, _>("theme", move |value| {
                observed.lock().unwrap().push(value.is_err());
            })
            .unwrap();
        store.set("theme", &42).unwrap();

        assert_eq!(*results.lock().unwrap(), vec![false, true]);
    }

    #[test]
    fn sqlite_keys_reports_query_failures() {
        let directory = tempdir().unwrap();
        let store = SqliteKvStore::open_at(directory.path().join("preferences.sqlite3")).unwrap();
        store
            .connection
            .lock()
            .execute("DROP TABLE kv_entries", [])
            .unwrap();

        assert!(store.keys().is_err());
    }

    #[test]
    fn sqlite_observe_reports_an_invalid_stored_value() {
        let directory = tempdir().unwrap();
        let store = SqliteKvStore::open_at(directory.path().join("preferences.sqlite3")).unwrap();
        store
            .connection
            .lock()
            .execute(
                "INSERT INTO kv_entries (key, value) VALUES (?1, ?2)",
                params!["theme", "not valid JSON"],
            )
            .unwrap();

        let result = store.observe::<String, _>("theme", |_| {
            panic!("the callback must not run when initial loading fails");
        });
        assert!(matches!(result, Err(Error::DeserializeValue { .. })));
    }

    #[test]
    fn sqlite_store_imports_legacy_json_values() {
        let directory = tempdir().unwrap();
        let json_path = directory.path().join("preferences.json");
        let sqlite_path = directory.path().join("preferences.sqlite3");
        let mut values = BTreeMap::new();
        values.insert(
            "preferences".to_string(),
            serde_json::to_value(Preferences {
                theme: "dark".to_string(),
                recent_ids: vec![1, 2, 3],
            })
            .unwrap(),
        );
        persist_values(&json_path, &values).unwrap();

        let store = SqliteKvStore::open_at(&sqlite_path).unwrap();
        store.import_legacy_json_if_empty(&json_path).unwrap();

        let restored = store.get::<Preferences>("preferences").unwrap();
        assert_eq!(
            restored,
            Some(Preferences {
                theme: "dark".to_string(),
                recent_ids: vec![1, 2, 3],
            })
        );
    }

    #[test]
    fn json_store_rolls_memory_back_when_persistence_fails() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        let store = JsonKvStore::open_at(&path).unwrap();

        std::fs::create_dir(&path).unwrap();
        assert!(store.set("theme", &"dark").is_err());
        assert_eq!(store.get::<String>("theme").unwrap(), None);

        std::fs::remove_dir(&path).unwrap();
        store.set("theme", &"dark").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(store.remove("theme").is_err());
        assert_eq!(
            store.get::<String>("theme").unwrap().as_deref(),
            Some("dark")
        );
    }

    #[test]
    fn json_store_size_limit_is_checked_without_allocation() {
        ensure_json_store_size(MAX_JSON_STORE_BYTES).unwrap();
        assert!(ensure_json_store_size(MAX_JSON_STORE_BYTES + 1).is_err());
    }
}
