use anyhow::Result;
use std::collections::HashMap;

const MAX_CATALOG_JSON_BYTES: usize = 16 * 1024 * 1024;

/// A collection of localized strings for a specific locale.
#[derive(Debug, Clone)]
pub struct StringCatalog {
    locale: String,
    strings: HashMap<String, String>,
}

impl StringCatalog {
    /// Creates a new empty string catalog for the given locale.
    pub fn new(locale: impl Into<String>) -> Self {
        Self {
            locale: locale.into(),
            strings: HashMap::new(),
        }
    }

    /// Creates a string catalog from a JSON string containing key-value pairs.
    pub fn from_json(locale: &str, json: &str) -> Result<Self> {
        validate_catalog_size(json.len())?;
        let strings: HashMap<String, String> = serde_json::from_str(json)?;
        Ok(Self {
            locale: locale.to_string(),
            strings,
        })
    }
}

fn validate_catalog_size(size: usize) -> Result<()> {
    anyhow::ensure!(
        size <= MAX_CATALOG_JSON_BYTES,
        "catalog JSON exceeds the {MAX_CATALOG_JSON_BYTES} byte limit"
    );
    Ok(())
}

impl StringCatalog {
    /// Returns the locale identifier for this catalog.
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Looks up a translated string by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.strings.get(key).map(|s| s.as_str())
    }

    /// Looks up a translated string by key, returning a default if not found.
    pub fn get_or_default<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.strings.get(key).map(|s| s.as_str()).unwrap_or(default)
    }

    /// Inserts or updates a translated string.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.strings.insert(key.into(), value.into());
    }

    /// Returns the number of strings in the catalog.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Returns true if the catalog contains no strings.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// Returns all keys in the catalog in deterministic lexical order.
    pub fn keys(&self) -> Vec<&str> {
        let mut keys = self.strings.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_catalog() {
        let catalog = StringCatalog::new("en-US");
        assert_eq!(catalog.locale(), "en-US");
        assert!(catalog.is_empty());
        assert_eq!(catalog.len(), 0);
    }

    #[test]
    fn test_insert_and_get() {
        let mut catalog = StringCatalog::new("en-US");
        catalog.insert("greeting", "Hello");
        catalog.insert("farewell", "Goodbye");

        assert_eq!(catalog.get("greeting"), Some("Hello"));
        assert_eq!(catalog.get("farewell"), Some("Goodbye"));
        assert_eq!(catalog.get("missing"), None);
        assert_eq!(catalog.len(), 2);
    }

    #[test]
    fn test_get_or_default() {
        let mut catalog = StringCatalog::new("en-US");
        catalog.insert("greeting", "Hello");

        assert_eq!(catalog.get_or_default("greeting", "Hi"), "Hello");
        assert_eq!(catalog.get_or_default("missing", "fallback"), "fallback");
    }

    #[test]
    fn test_from_json() {
        let json = r#"{"greeting": "Hallo", "farewell": "Tschüss"}"#;
        let catalog = StringCatalog::from_json("de-DE", json).unwrap();

        assert_eq!(catalog.locale(), "de-DE");
        assert_eq!(catalog.get("greeting"), Some("Hallo"));
        assert_eq!(catalog.get("farewell"), Some("Tschüss"));
        assert_eq!(catalog.len(), 2);
    }

    #[test]
    fn test_from_json_invalid() {
        let result = StringCatalog::from_json("en-US", "not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_keys() {
        let mut catalog = StringCatalog::new("en-US");
        catalog.insert("a", "A");
        catalog.insert("b", "B");

        assert_eq!(catalog.keys(), vec!["a", "b"]);
    }

    #[test]
    fn rejects_oversized_catalogs_without_parsing() {
        assert!(validate_catalog_size(MAX_CATALOG_JSON_BYTES + 1).is_err());
    }
}
