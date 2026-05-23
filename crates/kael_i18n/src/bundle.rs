use anyhow::{bail, Result};
use std::collections::HashMap;

use crate::catalog::StringCatalog;

/// Manages multiple locale catalogs with active/fallback locale selection.
#[derive(Debug)]
pub struct LocaleBundle {
    catalogs: HashMap<String, StringCatalog>,
    active_locale: String,
    fallback_locale: String,
}

impl LocaleBundle {
    /// Creates a new bundle with the given fallback locale.
    pub fn new(fallback_locale: impl Into<String>) -> Self {
        let fallback = fallback_locale.into();
        Self {
            catalogs: HashMap::new(),
            active_locale: fallback.clone(),
            fallback_locale: fallback,
        }
    }

    /// Adds a string catalog to the bundle, keyed by its locale.
    pub fn add_catalog(&mut self, catalog: StringCatalog) {
        let locale = catalog.locale().to_string();
        self.catalogs.insert(locale, catalog);
    }

    /// Sets the active locale. Returns an error if no catalog exists for the locale.
    pub fn set_active(&mut self, locale: impl Into<String>) -> Result<()> {
        let locale = locale.into();
        if !self.catalogs.contains_key(&locale) {
            bail!("No catalog found for locale '{}'", locale);
        }
        self.active_locale = locale;
        Ok(())
    }

    /// Returns the currently active locale identifier.
    pub fn active_locale(&self) -> &str {
        &self.active_locale
    }

    /// Translates a key using the active locale, falling back to the fallback locale.
    ///
    /// Returns the key itself if no translation is found in either catalog.
    pub fn translate<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(catalog) = self.catalogs.get(&self.active_locale)
            && let Some(value) = catalog.get(key)
        {
            return value;
        }

        if self.active_locale != self.fallback_locale
            && let Some(catalog) = self.catalogs.get(&self.fallback_locale)
            && let Some(value) = catalog.get(key)
        {
            return value;
        }

        key
    }

    /// Returns a sorted list of all available locale identifiers.
    pub fn available_locales(&self) -> Vec<&str> {
        let mut locales: Vec<&str> = self.catalogs.keys().map(|k| k.as_str()).collect();
        locales.sort();
        locales
    }

    /// Returns true if a catalog exists for the given locale.
    pub fn has_locale(&self, locale: &str) -> bool {
        self.catalogs.contains_key(locale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_en_catalog() -> StringCatalog {
        let mut catalog = StringCatalog::new("en-US");
        catalog.insert("greeting", "Hello");
        catalog.insert("farewell", "Goodbye");
        catalog.insert("app.name", "My App");
        catalog
    }

    fn make_de_catalog() -> StringCatalog {
        let mut catalog = StringCatalog::new("de-DE");
        catalog.insert("greeting", "Hallo");
        catalog.insert("farewell", "Tschüss");
        catalog
    }

    #[test]
    fn test_new_bundle() {
        let bundle = LocaleBundle::new("en-US");
        assert_eq!(bundle.active_locale(), "en-US");
        assert!(bundle.available_locales().is_empty());
    }

    #[test]
    fn test_add_and_translate() {
        let mut bundle = LocaleBundle::new("en-US");
        bundle.add_catalog(make_en_catalog());

        assert_eq!(bundle.translate("greeting"), "Hello");
        assert_eq!(bundle.translate("farewell"), "Goodbye");
    }

    #[test]
    fn test_translate_missing_key() {
        let mut bundle = LocaleBundle::new("en-US");
        bundle.add_catalog(make_en_catalog());

        assert_eq!(bundle.translate("nonexistent"), "nonexistent");
    }

    #[test]
    fn test_set_active_locale() {
        let mut bundle = LocaleBundle::new("en-US");
        bundle.add_catalog(make_en_catalog());
        bundle.add_catalog(make_de_catalog());

        bundle.set_active("de-DE").unwrap();
        assert_eq!(bundle.active_locale(), "de-DE");
        assert_eq!(bundle.translate("greeting"), "Hallo");
    }

    #[test]
    fn test_set_active_unknown_locale() {
        let mut bundle = LocaleBundle::new("en-US");
        let result = bundle.set_active("fr-FR");
        assert!(result.is_err());
    }

    #[test]
    fn test_fallback_translation() {
        let mut bundle = LocaleBundle::new("en-US");
        bundle.add_catalog(make_en_catalog());
        bundle.add_catalog(make_de_catalog());

        bundle.set_active("de-DE").unwrap();

        assert_eq!(bundle.translate("greeting"), "Hallo");
        assert_eq!(bundle.translate("app.name"), "My App");
    }

    #[test]
    fn test_available_locales() {
        let mut bundle = LocaleBundle::new("en-US");
        bundle.add_catalog(make_en_catalog());
        bundle.add_catalog(make_de_catalog());

        let locales = bundle.available_locales();
        assert_eq!(locales, vec!["de-DE", "en-US"]);
    }

    #[test]
    fn test_has_locale() {
        let mut bundle = LocaleBundle::new("en-US");
        bundle.add_catalog(make_en_catalog());

        assert!(bundle.has_locale("en-US"));
        assert!(!bundle.has_locale("fr-FR"));
    }
}
