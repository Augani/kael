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
    language: String,
}

impl PluralRules {
    /// Creates plural rules for the given locale.
    pub fn new(locale: impl Into<String>) -> Self {
        let locale = locale.into();
        Self {
            language: locale
                .split(['-', '_'])
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase(),
        }
    }

    /// Selects the CLDR-style cardinal plural category for an integer count.
    pub fn select(&self, count: u64) -> PluralCategory {
        let modulo_10 = count % 10;
        let modulo_100 = count % 100;
        match self.language.as_str() {
            "ar" => match count {
                0 => PluralCategory::Zero,
                1 => PluralCategory::One,
                2 => PluralCategory::Two,
                _ if (3..=10).contains(&modulo_100) => PluralCategory::Few,
                _ if (11..=99).contains(&modulo_100) => PluralCategory::Many,
                _ => PluralCategory::Other,
            },
            "cs" | "sk" => match count {
                1 => PluralCategory::One,
                2..=4 => PluralCategory::Few,
                _ => PluralCategory::Other,
            },
            "fr" if count == 0 || count == 1 => PluralCategory::One,
            "pl" => {
                if count == 1 {
                    PluralCategory::One
                } else if (2..=4).contains(&modulo_10) && !(12..=14).contains(&modulo_100) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Many
                }
            }
            "ro" => {
                if count == 1 {
                    PluralCategory::One
                } else if count == 0 || (1..=19).contains(&modulo_100) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Other
                }
            }
            "ru" | "uk" => {
                if modulo_10 == 1 && modulo_100 != 11 {
                    PluralCategory::One
                } else if (2..=4).contains(&modulo_10) && !(12..=14).contains(&modulo_100) {
                    PluralCategory::Few
                } else if modulo_10 == 0
                    || (5..=9).contains(&modulo_10)
                    || (11..=14).contains(&modulo_100)
                {
                    PluralCategory::Many
                } else {
                    PluralCategory::Other
                }
            }
            "ja" | "ko" | "th" | "vi" | "zh" => PluralCategory::Other,
            _ if count == 1 => PluralCategory::One,
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
        assert_eq!(rules.select(0), PluralCategory::Other);
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

        assert_eq!(rules.format("items", 0, &catalog), "0 items");
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

    #[test]
    fn selects_arabic_and_slavic_categories() {
        let arabic = PluralRules::new("ar-SA");
        assert_eq!(arabic.select(0), PluralCategory::Zero);
        assert_eq!(arabic.select(2), PluralCategory::Two);
        assert_eq!(arabic.select(7), PluralCategory::Few);
        assert_eq!(arabic.select(15), PluralCategory::Many);
        assert_eq!(arabic.select(100), PluralCategory::Other);

        let russian = PluralRules::new("ru-RU");
        assert_eq!(russian.select(1), PluralCategory::One);
        assert_eq!(russian.select(2), PluralCategory::Few);
        assert_eq!(russian.select(5), PluralCategory::Many);
        assert_eq!(russian.select(11), PluralCategory::Many);
        assert_eq!(russian.select(21), PluralCategory::One);

        let japanese = PluralRules::new("ja-JP");
        assert_eq!(japanese.select(1), PluralCategory::Other);
    }
}
