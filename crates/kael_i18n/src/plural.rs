use crate::catalog::StringCatalog;

/// Grammatical plural category for number-dependent translations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluralCategory {
    /// Exact zero quantity.
    Zero,
    /// Singular quantity (exactly one).
    One,
    /// Dual quantity (exactly two).
    Two,
    /// Small quantity (language-specific).
    Few,
    /// Large quantity (language-specific).
    Many,
    /// General plural form.
    Other,
}

/// Applies locale-aware pluralization rules to select the correct string form.
#[derive(Debug, Clone)]
pub struct PluralRules {
    #[allow(dead_code)]
    locale: String,
}

impl PluralRules {
    /// Creates plural rules for the given locale.
    pub fn new(locale: impl Into<String>) -> Self {
        Self {
            locale: locale.into(),
        }
    }

    /// Selects the plural category for a given count using English-style rules.
    pub fn select(&self, count: u64) -> PluralCategory {
        match count {
            0 => PluralCategory::Zero,
            1 => PluralCategory::One,
            _ => PluralCategory::Other,
        }
    }

    /// Formats a pluralized string by looking up category-suffixed keys in the catalog.
    ///
    /// Looks for keys like `"{key}.zero"`, `"{key}.one"`, `"{key}.other"` in the catalog.
    /// Falls back to `"{key}.other"`, then `"{key}"`, then the raw key itself.
    pub fn format(&self, key: &str, count: u64, catalog: &StringCatalog) -> String {
        let category = self.select(count);
        let suffix = match category {
            PluralCategory::Zero => "zero",
            PluralCategory::One => "one",
            PluralCategory::Two => "two",
            PluralCategory::Few => "few",
            PluralCategory::Many => "many",
            PluralCategory::Other => "other",
        };

        let specific_key = format!("{}.{}", key, suffix);
        if let Some(value) = catalog.get(&specific_key) {
            return value.replace("{count}", &count.to_string());
        }

        let other_key = format!("{}.other", key);
        if let Some(value) = catalog.get(&other_key) {
            return value.replace("{count}", &count.to_string());
        }

        if let Some(value) = catalog.get(key) {
            return value.replace("{count}", &count.to_string());
        }

        key.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_zero() {
        let rules = PluralRules::new("en-US");
        assert_eq!(rules.select(0), PluralCategory::Zero);
    }

    #[test]
    fn test_select_one() {
        let rules = PluralRules::new("en-US");
        assert_eq!(rules.select(1), PluralCategory::One);
    }

    #[test]
    fn test_select_other() {
        let rules = PluralRules::new("en-US");
        assert_eq!(rules.select(2), PluralCategory::Other);
        assert_eq!(rules.select(5), PluralCategory::Other);
        assert_eq!(rules.select(100), PluralCategory::Other);
    }

    #[test]
    fn test_format_with_plural_keys() {
        let mut catalog = StringCatalog::new("en-US");
        catalog.insert("items.zero", "No items");
        catalog.insert("items.one", "{count} item");
        catalog.insert("items.other", "{count} items");

        let rules = PluralRules::new("en-US");

        assert_eq!(rules.format("items", 0, &catalog), "No items");
        assert_eq!(rules.format("items", 1, &catalog), "1 item");
        assert_eq!(rules.format("items", 5, &catalog), "5 items");
    }

    #[test]
    fn test_format_fallback_to_other() {
        let mut catalog = StringCatalog::new("en-US");
        catalog.insert("items.other", "{count} items");

        let rules = PluralRules::new("en-US");
        assert_eq!(rules.format("items", 0, &catalog), "0 items");
    }

    #[test]
    fn test_format_fallback_to_base_key() {
        let mut catalog = StringCatalog::new("en-US");
        catalog.insert("items", "{count} item(s)");

        let rules = PluralRules::new("en-US");
        assert_eq!(rules.format("items", 3, &catalog), "3 item(s)");
    }

    #[test]
    fn test_format_missing_key() {
        let catalog = StringCatalog::new("en-US");
        let rules = PluralRules::new("en-US");
        assert_eq!(rules.format("missing", 1, &catalog), "missing");
    }
}
