use anyhow::{Result, bail};
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
        let Some(resolved) = self.resolve_locale(&locale) else {
            bail!("No catalog found for locale '{locale}'");
        };
        self.active_locale = resolved.to_string();
        Ok(())
    }

    /// Sets the fallback locale after resolving an exact or language-compatible catalog.
    pub fn set_fallback(&mut self, locale: impl Into<String>) -> Result<()> {
        let locale = locale.into();
        let Some(resolved) = self.resolve_locale(&locale) else {
            bail!("No catalog found for locale '{locale}'");
        };
        self.fallback_locale = resolved.to_string();
        Ok(())
    }

    /// Returns the currently active locale identifier.
    pub fn active_locale(&self) -> &str {
        &self.active_locale
    }

    /// Returns the currently configured fallback locale identifier.
    pub fn fallback_locale(&self) -> &str {
        &self.fallback_locale
    }

    /// Translates a key using the active locale, falling back to the fallback locale.
    ///
    /// Returns the key itself if no translation is found in either catalog.
    pub fn translate<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(active_locale) = self.resolve_locale(&self.active_locale)
            && let Some(catalog) = self.catalogs.get(active_locale)
            && let Some(value) = catalog.get(key)
        {
            return value;
        }

        if self.active_locale != self.fallback_locale
            && let Some(fallback_locale) = self.resolve_locale(&self.fallback_locale)
            && let Some(catalog) = self.catalogs.get(fallback_locale)
            && let Some(value) = catalog.get(key)
        {
            return value;
        }

        key
    }

    /// Translates a key and substitutes named `{placeholder}` values.
    ///
    /// Substitution is performed in one pass: placeholder-like text inside an
    /// argument value is never interpreted as another placeholder.
    pub fn translate_with_args(&self, key: &str, arguments: &[(&str, &str)]) -> String {
        substitute_named(self.translate(key), arguments)
    }

    /// Returns a sorted list of all available locale identifiers.
    pub fn available_locales(&self) -> Vec<&str> {
        let mut locales: Vec<&str> = self.catalogs.keys().map(|k| k.as_str()).collect();
        locales.sort();
        locales
    }

    /// Returns true if a catalog exists for the given locale.
    pub fn has_locale(&self, locale: &str) -> bool {
        self.resolve_locale(locale).is_some()
    }

    fn resolve_locale(&self, requested: &str) -> Option<&str> {
        let requested = normalize_locale(requested);
        if requested.is_empty() {
            return None;
        }
        let mut locales = self.catalogs.keys().map(String::as_str).collect::<Vec<_>>();
        locales.sort_unstable();
        locales
            .iter()
            .copied()
            .find(|locale| normalize_locale(locale) == requested)
            .or_else(|| {
                let language = requested.split('-').next().unwrap_or_default();
                locales.into_iter().find(|locale| {
                    normalize_locale(locale)
                        .split('-')
                        .next()
                        .is_some_and(|candidate| candidate == language)
                })
            })
    }
}

fn substitute_named(template: &str, arguments: &[(&str, &str)]) -> String {
    let mut output = String::with_capacity(template.len());
    let mut remaining = template;

    while let Some(open) = remaining.find('{') {
        output.push_str(&remaining[..open]);
        let after_open = &remaining[open + 1..];
        let Some(close) = after_open.find('}') else {
            output.push_str(&remaining[open..]);
            return output;
        };
        let name = &after_open[..close];

        if !name.is_empty() && !name.contains('{') {
            if let Some((_, value)) = arguments.iter().find(|(candidate, _)| *candidate == name) {
                output.push_str(value);
            } else {
                output.push('{');
                output.push_str(name);
                output.push('}');
            }
            remaining = &after_open[close + 1..];
        } else {
            output.push('{');
            remaining = after_open;
        }
    }

    output.push_str(remaining);
    output
}

fn normalize_locale(locale: &str) -> String {
    locale.trim().replace('_', "-").to_ascii_lowercase()
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

    #[test]
    fn resolves_compatible_locales_and_supports_named_arguments() {
        let mut bundle = LocaleBundle::new("en-US");
        let mut english = make_en_catalog();
        english.insert("welcome", "Welcome, {name}");
        bundle.add_catalog(english);
        bundle.add_catalog(make_de_catalog());

        bundle.set_active("de_AT").unwrap();
        assert_eq!(bundle.active_locale(), "de-DE");
        bundle.set_fallback("EN_us").unwrap();
        assert_eq!(bundle.fallback_locale(), "en-US");
        assert_eq!(
            bundle.translate_with_args("welcome", &[("name", "Ada")]),
            "Welcome, Ada"
        );
        assert!(bundle.has_locale("de-CH"));
    }

    #[test]
    fn named_arguments_are_substituted_once_without_cascading() {
        assert_eq!(
            substitute_named(
                "{first} / {second} / {missing}",
                &[("first", "{second}"), ("second", "done")]
            ),
            "{second} / done / {missing}"
        );
        assert_eq!(
            substitute_named("unclosed {name", &[("name", "Ada")]),
            "unclosed {name"
        );
        assert_eq!(
            substitute_named("literal {}", &[("", "hidden")]),
            "literal {}"
        );
    }
}
