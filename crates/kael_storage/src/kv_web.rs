//! Browser key-value storage backed by one atomic `localStorage` entry per app.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::Arc,
};

use parking_lot::Mutex;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use wasm_bindgen::JsValue;
use web_sys::Storage;

use crate::{Error, Result, Subscription};

type ObserverCallback = Arc<dyn Fn(Option<Value>) + Send + Sync + 'static>;
type Observer = Arc<QueuedObserver>;

const MAX_BROWSER_JSON_STORE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_KEY_BYTES: usize = 4 * 1024;

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

    /// Observes changes made through this store instance and immediately reports the current value.
    ///
    /// Browser tabs and independently opened store instances see each other's persisted values on
    /// their next operation, but observer callbacks are intentionally scoped to the instance that
    /// created them.
    fn observe<T, F>(&self, key: &str, callback: F) -> Result<Subscription>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(Result<Option<T>>) + Send + Sync + 'static;
}

/// A browser `localStorage`-backed typed key-value store.
///
/// The complete map is stored in one browser entry, so every successful mutation is visible
/// atomically. This backend is intended for settings and small metadata. Use [`crate::BlobStore`]
/// for document bodies, binary assets, and other large values.
#[derive(Clone)]
pub struct BrowserKvStore {
    storage_key: String,
    state: Arc<Mutex<KvState>>,
}

/// The default key-value store for browser WebAssembly.
pub type PlatformKvStore = BrowserKvStore;

impl std::fmt::Debug for BrowserKvStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserKvStore")
            .field("storage_key", &self.storage_key)
            .finish_non_exhaustive()
    }
}

impl BrowserKvStore {
    /// Opens the persistent browser settings store for an application identifier.
    pub fn open(app_id: &str) -> Result<Self> {
        crate::platform::validate_storage_identifier(app_id)?;
        Self::open_namespace(format!("kael:kv:v1:{app_id}"))
    }

    /// Opens a persistent browser settings namespace.
    ///
    /// Namespace identifiers use the same portable validation as application identifiers. This
    /// is useful when one application needs isolated settings domains.
    pub fn open_named(app_id: &str, namespace: &str) -> Result<Self> {
        crate::platform::validate_storage_identifier(app_id)?;
        crate::platform::validate_storage_identifier(namespace)?;
        Self::open_namespace(format!("kael:kv:v1:{app_id}:{namespace}"))
    }

    fn open_namespace(storage_key: String) -> Result<Self> {
        let values = load_values(&browser_storage()?, &storage_key)?;
        Ok(Self {
            storage_key,
            state: Arc::new(Mutex::new(KvState {
                values,
                observers: HashMap::new(),
                next_observer_id: 0,
            })),
        })
    }

    fn refresh(state: &mut KvState, storage: &Storage, storage_key: &str) -> Result<()> {
        state.values = load_values(storage, storage_key)?;
        Ok(())
    }

    fn notify(observers: Vec<Observer>, value: Option<Value>) {
        let mut panic_payload = None;
        for observer in observers {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| observer.notify(value.clone())))
            {
                panic_payload.get_or_insert(payload);
            }
        }
        if let Some(payload) = panic_payload {
            resume_unwind(payload);
        }
    }
}

impl KvStore for BrowserKvStore {
    fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        validate_key(key)?;
        let storage = browser_storage()?;
        let mut state = self.state.lock();
        Self::refresh(&mut state, &storage, &self.storage_key)?;
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
        validate_key(key)?;
        let serialized = serde_json::to_value(value).map_err(|source| Error::SerializeValue {
            key: key.to_string(),
            source,
        })?;
        let storage = browser_storage()?;

        let observers = {
            let mut state = self.state.lock();
            Self::refresh(&mut state, &storage, &self.storage_key)?;
            if state.values.get(key) == Some(&serialized) {
                return Ok(());
            }

            state.values.insert(key.to_string(), serialized.clone());
            persist_values(&storage, &self.storage_key, &state.values)?;
            observers_for_key(&state.observers, key)
        };

        Self::notify(observers, Some(serialized));
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<bool> {
        validate_key(key)?;
        let storage = browser_storage()?;
        let observers = {
            let mut state = self.state.lock();
            Self::refresh(&mut state, &storage, &self.storage_key)?;
            if state.values.remove(key).is_none() {
                return Ok(false);
            }
            persist_values(&storage, &self.storage_key, &state.values)?;
            observers_for_key(&state.observers, key)
        };

        Self::notify(observers, None);
        Ok(true)
    }

    fn keys(&self) -> Result<Vec<String>> {
        let storage = browser_storage()?;
        let mut state = self.state.lock();
        Self::refresh(&mut state, &storage, &self.storage_key)?;
        Ok(state.values.keys().cloned().collect())
    }

    fn observe<T, F>(&self, key: &str, callback: F) -> Result<Subscription>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(Result<Option<T>>) + Send + Sync + 'static,
    {
        validate_key(key)?;
        let callback = typed_observer(key, callback);
        let storage = browser_storage()?;
        let key = key.to_string();
        let (observer_id, observer) = {
            let mut state = self.state.lock();
            Self::refresh(&mut state, &storage, &self.storage_key)?;
            let observer_id = state.allocate_observer_id();
            let observer = QueuedObserver::pending(callback, state.values.get(&key).cloned());
            state
                .observers
                .entry(key.clone())
                .or_default()
                .insert(observer_id, observer.clone());
            (observer_id, observer)
        };

        let state = self.state.clone();
        let unsubscribe_key = key;
        let subscription = Subscription::new(move || {
            let mut state = state.lock();
            if let Some(observers) = state.observers.get_mut(&unsubscribe_key) {
                observers.remove(&observer_id);
                if observers.is_empty() {
                    state.observers.remove(&unsubscribe_key);
                }
            }
        });
        observer.activate();
        Ok(subscription)
    }
}

struct ObserverDelivery {
    queue: VecDeque<Option<Value>>,
    delivering: bool,
}

struct QueuedObserver {
    callback: ObserverCallback,
    delivery: Mutex<ObserverDelivery>,
}

impl QueuedObserver {
    fn pending(callback: ObserverCallback, initial_value: Option<Value>) -> Observer {
        Arc::new(Self {
            callback,
            delivery: Mutex::new(ObserverDelivery {
                queue: VecDeque::from([initial_value]),
                delivering: true,
            }),
        })
    }

    fn activate(&self) {
        self.drain();
    }

    fn notify(&self, value: Option<Value>) {
        let should_drain = {
            let mut delivery = self.delivery.lock();
            delivery.queue.push_back(value);
            if delivery.delivering {
                false
            } else {
                delivery.delivering = true;
                true
            }
        };
        if should_drain {
            self.drain();
        }
    }

    fn drain(&self) {
        let mut reset = DeliveryReset {
            observer: self,
            armed: true,
        };
        loop {
            let value = {
                let mut delivery = self.delivery.lock();
                let Some(value) = delivery.queue.pop_front() else {
                    delivery.delivering = false;
                    reset.armed = false;
                    return;
                };
                value
            };
            (self.callback)(value);
        }
    }
}

struct DeliveryReset<'a> {
    observer: &'a QueuedObserver,
    armed: bool,
}

impl Drop for DeliveryReset<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.observer.delivery.lock().delivering = false;
        }
    }
}

struct KvState {
    values: BTreeMap<String, Value>,
    observers: HashMap<String, BTreeMap<usize, Observer>>,
    next_observer_id: usize,
}

impl KvState {
    fn allocate_observer_id(&mut self) -> usize {
        loop {
            let candidate = self.next_observer_id;
            self.next_observer_id = self.next_observer_id.checked_add(1).unwrap_or(0);
            if self
                .observers
                .values()
                .all(|observers| !observers.contains_key(&candidate))
            {
                return candidate;
            }
        }
    }
}

fn browser_storage() -> Result<Storage> {
    web_sys::window()
        .ok_or_else(|| browser_error("localStorage", "browser window is unavailable"))?
        .local_storage()
        .map_err(|error| browser_js_error("localStorage", &error))?
        .ok_or_else(|| browser_error("localStorage", "persistent storage is disabled"))
}

fn load_values(storage: &Storage, storage_key: &str) -> Result<BTreeMap<String, Value>> {
    let Some(contents) = storage
        .get_item(storage_key)
        .map_err(|error| browser_js_error("localStorage.getItem", &error))?
    else {
        return Ok(BTreeMap::new());
    };
    ensure_store_size(contents.len().try_into().unwrap_or(u64::MAX))?;
    let values: BTreeMap<String, Value> = serde_json::from_str(&contents)?;
    for key in values.keys() {
        validate_key(key)?;
    }
    Ok(values)
}

fn persist_values(
    storage: &Storage,
    storage_key: &str,
    values: &BTreeMap<String, Value>,
) -> Result<()> {
    for key in values.keys() {
        validate_key(key)?;
    }
    let contents = serde_json::to_string(values)?;
    ensure_store_size(contents.len().try_into().unwrap_or(u64::MAX))?;
    storage
        .set_item(storage_key, &contents)
        .map_err(|error| browser_js_error("localStorage.setItem", &error))
}

fn ensure_store_size(actual: u64) -> Result<()> {
    if actual > MAX_BROWSER_JSON_STORE_BYTES {
        Err(Error::JsonStoreTooLarge {
            actual,
            limit: MAX_BROWSER_JSON_STORE_BYTES,
        })
    } else {
        Ok(())
    }
}

fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control) {
        Err(Error::InvalidStorageKey)
    } else {
        Ok(())
    }
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

fn typed_observer<T, F>(key: &str, callback: F) -> ObserverCallback
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

fn browser_error(operation: &'static str, message: impl Into<String>) -> Error {
    Error::BrowserStorage {
        operation,
        message: message.into(),
    }
}

fn browser_js_error(operation: &'static str, error: &JsValue) -> Error {
    browser_error(
        operation,
        error.as_string().unwrap_or_else(|| format!("{error:?}")),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        BrowserKvStore, KvStore, MAX_BROWSER_JSON_STORE_BYTES, ensure_store_size, validate_key,
    };
    use crate::Error;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[test]
    fn validates_browser_bounds_without_dom_access() {
        assert!(validate_key("theme").is_ok());
        assert!(matches!(validate_key(""), Err(Error::InvalidStorageKey)));
        assert!(ensure_store_size(MAX_BROWSER_JSON_STORE_BYTES).is_ok());
        assert!(matches!(
            ensure_store_size(MAX_BROWSER_JSON_STORE_BYTES + 1),
            Err(Error::JsonStoreTooLarge { .. })
        ));
    }

    #[wasm_bindgen_test]
    fn local_storage_round_trips_across_store_instances() {
        let app_id = format!("kael.storage.test.{}", js_sys::Date::now());
        let first = BrowserKvStore::open(&app_id).unwrap();
        first.set("theme", &"dark").unwrap();

        let second = BrowserKvStore::open(&app_id).unwrap();
        assert_eq!(
            second.get::<String>("theme").unwrap().as_deref(),
            Some("dark")
        );
        assert_eq!(second.keys().unwrap(), vec!["theme"]);
        assert!(second.remove("theme").unwrap());
        assert_eq!(first.get::<String>("theme").unwrap(), None);
    }
}
