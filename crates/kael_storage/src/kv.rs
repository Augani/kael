//! Typed key-value storage backends.

use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use crate::{platform, Error, Result, Subscription};

type Observer = Arc<dyn Fn(Option<Value>) + Send + Sync + 'static>;

struct KvState {
    values: BTreeMap<String, Value>,
    observers: HashMap<String, BTreeMap<usize, Observer>>,
    next_observer_id: usize,
    generation: u64,
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
    fn keys(&self) -> Vec<String>;

    /// Observes changes to a key.
    fn observe<T, F>(&self, key: &str, callback: F) -> Subscription
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(Option<T>) + Send + Sync + 'static;
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

        if let Some(parent) = path.parent() {
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
                generation: 0,
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

        if let Some(parent) = path.parent() {
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

        loop {
            let (next_values, observers, generation) = {
                let state = self.state.lock();

                if state.values.get(key) == Some(&serialized) {
                    return Ok(());
                }

                let mut next_values = state.values.clone();
                next_values.insert(key.to_string(), serialized.clone());

                (
                    next_values,
                    state
                        .observers
                        .get(key)
                        .map(|observers| observers.values().cloned().collect())
                        .unwrap_or_default(),
                    state.generation,
                )
            };

            persist_values(&self.path, &next_values)?;

            let committed = {
                let mut state = self.state.lock();
                if state.generation != generation {
                    false
                } else {
                    state.values = next_values;
                    state.generation += 1;
                    true
                }
            };

            if committed {
                Self::notify(observers, Some(serialized));
                return Ok(());
            }
        }
    }

    fn remove(&self, key: &str) -> Result<bool> {
        loop {
            let (next_values, observers, generation) = {
                let state = self.state.lock();

                if !state.values.contains_key(key) {
                    return Ok(false);
                }

                let mut next_values = state.values.clone();
                next_values.remove(key);

                (
                    next_values,
                    state
                        .observers
                        .get(key)
                        .map(|observers| observers.values().cloned().collect())
                        .unwrap_or_default(),
                    state.generation,
                )
            };

            persist_values(&self.path, &next_values)?;

            let committed = {
                let mut state = self.state.lock();
                if state.generation != generation {
                    false
                } else {
                    state.values = next_values;
                    state.generation += 1;
                    true
                }
            };

            if committed {
                Self::notify(observers, None);
                return Ok(true);
            }
        }
    }

    fn keys(&self) -> Vec<String> {
        self.state.lock().values.keys().cloned().collect()
    }

    fn observe<T, F>(&self, key: &str, callback: F) -> Subscription
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(Option<T>) + Send + Sync + 'static,
    {
        let callback = Arc::new(callback);
        let observer: Observer = {
            let callback = callback.clone();
            Arc::new(move |value| {
                let deserialized = value.and_then(|value| serde_json::from_value(value).ok());
                callback(deserialized);
            })
        };

        let key = key.to_string();
        let observer_id;
        let current_value = {
            let mut state = self.state.lock();
            observer_id = state.next_observer_id;
            state.next_observer_id += 1;
            state
                .observers
                .entry(key.clone())
                .or_default()
                .insert(observer_id, observer.clone());
            state.values.get(&key).cloned()
        };

        observer(current_value);

        let state = self.state.clone();
        let unsubscribe_key = key.clone();
        Subscription::new(move || {
            let mut state = state.lock();
            if let Some(observers) = state.observers.get_mut(&unsubscribe_key) {
                observers.remove(&observer_id);
                if observers.is_empty() {
                    state.observers.remove(&unsubscribe_key);
                }
            }
        })
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

    fn keys(&self) -> Vec<String> {
        let connection = self.connection.lock();
        let Ok(mut statement) = connection.prepare("SELECT key FROM kv_entries ORDER BY key")
        else {
            return Vec::new();
        };
        let Ok(rows) = statement.query_map([], |row| row.get::<_, String>(0)) else {
            return Vec::new();
        };

        rows.filter_map(|row| row.ok()).collect()
    }

    fn observe<T, F>(&self, key: &str, callback: F) -> Subscription
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(Option<T>) + Send + Sync + 'static,
    {
        let callback = Arc::new(callback);
        let observer: Observer = {
            let callback = callback.clone();
            Arc::new(move |value| {
                let deserialized = value.and_then(|value| serde_json::from_value(value).ok());
                callback(deserialized);
            })
        };

        let key = key.to_string();
        let (observer_id, current_value) = {
            let connection = self.connection.lock();
            let current_value = load_value_from_connection(&connection, &key).ok().flatten();
            let mut state = self.observers.lock();
            let observer_id = state.next_observer_id;
            state.next_observer_id += 1;
            state
                .observers
                .entry(key.clone())
                .or_default()
                .insert(observer_id, observer.clone());
            (observer_id, current_value)
        };

        observer(current_value);

        let observers = self.observers.clone();
        let unsubscribe_key = key.clone();
        Subscription::new(move || {
            let mut state = observers.lock();
            if let Some(observers) = state.observers.get_mut(&unsubscribe_key) {
                observers.remove(&observer_id);
                if observers.is_empty() {
                    state.observers.remove(&unsubscribe_key);
                }
            }
        })
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
        .map(|json| serde_json::from_str(&json).map_err(Error::from))
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

fn load_values(path: &Path) -> Result<BTreeMap<String, Value>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let contents = std::fs::read(path).map_err(|source| Error::io(path.to_path_buf(), source))?;
    Ok(serde_json::from_slice(&contents)?)
}

fn persist_values(path: &Path, values: &BTreeMap<String, Value>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|source| Error::io(parent.to_path_buf(), source))?;
    }

    let temp_path = temporary_path(path);
    let contents = serde_json::to_vec(values)?;
    let mut file =
        File::create(&temp_path).map_err(|source| Error::io(temp_path.clone(), source))?;
    file.write_all(&contents)
        .map_err(|source| Error::io(temp_path.clone(), source))?;
    file.flush()
        .map_err(|source| Error::io(temp_path.clone(), source))?;
    file.sync_all()
        .map_err(|source| Error::io(temp_path.clone(), source))?;
    std::fs::rename(&temp_path, path).map_err(|source| Error::io(path.to_path_buf(), source))?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("preferences.json");
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    path.with_file_name(format!(
        "{file_name}.{}.{}.tmp",
        std::process::id(),
        unique_suffix
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    use super::{persist_values, JsonKvStore, KvStore, SqliteKvStore};

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
        assert_eq!(store.keys(), vec!["preferences".to_string()]);
    }

    #[test]
    fn notifies_observers_on_registration_and_change() {
        let directory = tempdir().unwrap();
        let store = JsonKvStore::open_at(directory.path().join("preferences.json")).unwrap();
        let values = Arc::new(Mutex::new(Vec::new()));
        let observed = values.clone();

        let _subscription = store.observe::<String, _>("theme", move |value| {
            observed.lock().unwrap().push(value);
        });

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
        assert_eq!(store.keys(), vec!["preferences".to_string()]);
    }

    #[test]
    fn sqlite_store_notifies_observers_on_registration_and_change() {
        let directory = tempdir().unwrap();
        let store = SqliteKvStore::open_at(directory.path().join("preferences.sqlite3")).unwrap();
        let values = Arc::new(Mutex::new(Vec::new()));
        let observed = values.clone();

        let _subscription = store.observe::<String, _>("theme", move |value| {
            observed.lock().unwrap().push(value);
        });

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
        let _sub = store.observe::<String, _>("color", move |value| {
            recv_clone.lock().unwrap().push(value);
        });

        let values = received.lock().unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], Some("blue".to_string()));
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
}
