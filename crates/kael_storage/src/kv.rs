//! Typed JSON-backed key-value storage.

use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::Mutex;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{Error, Result, Subscription, platform};

type Observer = Arc<dyn Fn(Option<Value>) + Send + Sync + 'static>;

struct KvState {
    values: BTreeMap<String, Value>,
    observers: HashMap<String, BTreeMap<usize, Observer>>,
    next_observer_id: usize,
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

/// The default key-value store implementation for the current platform.
pub type PlatformKvStore = JsonKvStore;

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

            let mut next_values = state.values.clone();
            next_values.insert(key.to_string(), serialized.clone());
            persist_values(&self.path, &next_values)?;
            state.values = next_values;

            state
                .observers
                .get(key)
                .map(|observers| observers.values().cloned().collect())
                .unwrap_or_default()
        };

        Self::notify(observers, Some(serialized));
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<bool> {
        let (removed, observers) = {
            let mut state = self.state.lock();

            if !state.values.contains_key(key) {
                return Ok(false);
            }

            let mut next_values = state.values.clone();
            next_values.remove(key);
            persist_values(&self.path, &next_values)?;
            state.values = next_values;

            let observers = state
                .observers
                .get(key)
                .map(|observers| observers.values().cloned().collect())
                .unwrap_or_default();

            (true, observers)
        };

        Self::notify(observers, None);
        Ok(removed)
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

fn load_values(path: &Path) -> Result<BTreeMap<String, Value>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let contents =
        std::fs::read_to_string(path).map_err(|source| Error::io(path.to_path_buf(), source))?;
    Ok(serde_json::from_str(&contents)?)
}

fn persist_values(path: &Path, values: &BTreeMap<String, Value>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|source| Error::io(parent.to_path_buf(), source))?;
    }

    let temp_path = path.with_extension("json.tmp");
    let contents = serde_json::to_vec_pretty(values)?;
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    use super::{JsonKvStore, KvStore};

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
}
