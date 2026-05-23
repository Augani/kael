/// Formats numbers and dates according to locale-specific conventions.
#[derive(Debug, Clone)]
pub struct LocaleFormatter {
    locale: String,
    decimal_separator: char,
    thousands_separator: char,
    #[allow(dead_code)]
    date_format: String,
}

impl LocaleFormatter {
    /// Creates a locale formatter with default (en-US) separators.
    pub fn new(locale: impl Into<String>) -> Self {
        Self {
            locale: locale.into(),
            decimal_separator: '.',
            thousands_separator: ',',
            date_format: "MM/dd/yyyy".to_string(),
        }
    }

    /// Creates a preconfigured formatter for well-known locales.
    ///
    /// Supports: en-US, de-DE, ja-JP, ar-SA. Other locales fall back to en-US defaults.
    pub fn for_locale(locale: &str) -> Self {
        match locale {
            "en-US" => Self {
                locale: locale.to_string(),
                decimal_separator: '.',
                thousands_separator: ',',
                date_format: "MM/dd/yyyy".to_string(),
            },
            "de-DE" => Self {
                locale: locale.to_string(),
                decimal_separator: ',',
                thousands_separator: '.',
                date_format: "dd.MM.yyyy".to_string(),
            },
            "ja-JP" => Self {
                locale: locale.to_string(),
                decimal_separator: '.',
                thousands_separator: ',',
                date_format: "yyyy/MM/dd".to_string(),
            },
            "ar-SA" => Self {
                locale: locale.to_string(),
                decimal_separator: '٫',
                thousands_separator: '٬',
                date_format: "dd/MM/yyyy".to_string(),
            },
            _ => Self::new(locale),
        }
    }

    /// Formats a floating-point number with locale-appropriate separators.
    pub fn format_number(&self, value: f64, decimal_places: usize) -> String {
        let factor = 10_u64
            .checked_pow(decimal_places as u32)
            .unwrap_or(u64::MAX);
        let scaled = (value.abs() * factor as f64).round() as u64;
        let negative = value.is_sign_negative() && scaled != 0;
        let integer_part = if decimal_places == 0 {
            scaled
        } else {
            scaled / factor
        };
        let formatted_integer = self.format_integer_inner(integer_part, negative);

        if decimal_places == 0 {
            return formatted_integer;
        }

        let decimal_part = scaled % factor;
        let padded = format!("{:0>width$}", decimal_part, width = decimal_places);

        format!("{}{}{}", formatted_integer, self.decimal_separator, padded)
    }

    /// Formats an integer with locale-appropriate thousands separators.
    pub fn format_integer(&self, value: i64) -> String {
        self.format_integer_inner(value.unsigned_abs(), value < 0)
    }

    fn format_integer_inner(&self, abs_value: u64, negative: bool) -> String {
        let digits = abs_value.to_string();
        let mut result = String::with_capacity(digits.len() + digits.len() / 3 + 1);

        if negative {
            result.push('-');
        }

        for (idx, ch) in digits.chars().enumerate() {
            if idx > 0 && (digits.len() - idx).is_multiple_of(3) {
                result.push(self.thousands_separator);
            }
            result.push(ch);
        }

        result
    }

    /// Returns true if the locale uses right-to-left text direction.
    pub fn is_rtl(&self) -> bool {
        self.locale.starts_with("ar")
            || self.locale.starts_with("he")
            || self.locale.starts_with("fa")
    }

    /// Returns the locale identifier.
    pub fn locale(&self) -> &str {
        &self.locale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number_en_us() {
        let fmt = LocaleFormatter::for_locale("en-US");
        assert_eq!(fmt.format_number(1234.56, 2), "1,234.56");
        assert_eq!(fmt.format_number(0.5, 1), "0.5");
        assert_eq!(fmt.format_number(1000000.0, 0), "1,000,000");
    }

    #[test]
    fn test_format_number_de_de() {
        let fmt = LocaleFormatter::for_locale("de-DE");
        assert_eq!(fmt.format_number(1234.56, 2), "1.234,56");
        assert_eq!(fmt.format_number(1000000.0, 0), "1.000.000");
    }

    #[test]
    fn test_format_integer() {
        let fmt = LocaleFormatter::for_locale("en-US");
        assert_eq!(fmt.format_integer(1234567), "1,234,567");
        assert_eq!(fmt.format_integer(0), "0");
        assert_eq!(fmt.format_integer(-42), "-42");
        assert_eq!(fmt.format_integer(-1234), "-1,234");
    }

    #[test]
    fn test_format_number_ja_jp() {
        let fmt = LocaleFormatter::for_locale("ja-JP");
        assert_eq!(fmt.format_number(1234.5, 1), "1,234.5");
    }

    #[test]
    fn format_number_carries_fraction_rounding() {
        let fmt = LocaleFormatter::for_locale("en-US");
        assert_eq!(fmt.format_number(1.999, 2), "2.00");
        assert_eq!(fmt.format_number(-1.999, 2), "-2.00");
        assert_eq!(fmt.format_number(1.5, 0), "2");
    }

    #[test]
    fn test_format_number_ar_sa() {
        let fmt = LocaleFormatter::for_locale("ar-SA");
        assert_eq!(fmt.format_number(1234.56, 2), "1٬234٫56");
    }

    #[test]
    fn test_is_rtl() {
        assert!(LocaleFormatter::for_locale("ar-SA").is_rtl());
        assert!(!LocaleFormatter::for_locale("en-US").is_rtl());
        assert!(!LocaleFormatter::for_locale("de-DE").is_rtl());
        assert!(!LocaleFormatter::for_locale("ja-JP").is_rtl());
    }

    #[test]
    fn test_locale() {
        let fmt = LocaleFormatter::for_locale("de-DE");
        assert_eq!(fmt.locale(), "de-DE");
    }

    #[test]
    fn test_unknown_locale_defaults() {
        let fmt = LocaleFormatter::for_locale("fr-FR");
        assert_eq!(fmt.format_number(1234.56, 2), "1,234.56");
    }

    #[test]
    fn unsupported_locale_falls_back_gracefully() {
        let fmt = LocaleFormatter::for_locale("xx-YY");
        let output = fmt.format_number(1234.56, 2);
        assert!(!output.is_empty());
    }

    #[test]
    fn empty_locale_falls_back_gracefully() {
        let fmt = LocaleFormatter::for_locale("");
        let output = fmt.format_number(1234.56, 2);
        assert!(!output.is_empty());
    }

    #[test]
    fn snapshot_en_us() {
        let fmt = LocaleFormatter::for_locale("en-US");
        assert_eq!(fmt.format_number(1_000_000.0, 2), "1,000,000.00");
        assert_eq!(fmt.format_integer(9_999), "9,999");
        assert!(!fmt.is_rtl());
    }

    #[test]
    fn snapshot_de_de() {
        let fmt = LocaleFormatter::for_locale("de-DE");
        assert_eq!(fmt.format_number(1_000_000.0, 2), "1.000.000,00");
        assert_eq!(fmt.format_integer(9_999), "9.999");
        assert!(!fmt.is_rtl());
    }

    #[test]
    fn snapshot_ja_jp() {
        let fmt = LocaleFormatter::for_locale("ja-JP");
        assert_eq!(fmt.format_number(1_000_000.0, 2), "1,000,000.00");
        assert!(!fmt.is_rtl());
    }

    #[test]
    fn snapshot_ar_sa() {
        let fmt = LocaleFormatter::for_locale("ar-SA");
        assert_eq!(
            fmt.format_number(1_000_000.0, 2),
            "1\u{066c}000\u{066c}000\u{066b}00"
        );
        assert!(fmt.is_rtl());
    }

    #[test]
    fn test_format_small_numbers() {
        let fmt = LocaleFormatter::for_locale("en-US");
        assert_eq!(fmt.format_integer(1), "1");
        assert_eq!(fmt.format_integer(12), "12");
        assert_eq!(fmt.format_integer(123), "123");
        assert_eq!(fmt.format_integer(999), "999");
    }
}
