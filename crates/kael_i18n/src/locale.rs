/// Formats numbers and dates according to locale-specific conventions.
#[derive(Debug, Clone)]
pub struct LocaleFormatter {
    locale: String,
    decimal_separator: char,
    thousands_separator: char,
    date_format: String,
}

const MAX_DECIMAL_PLACES: usize = 18;

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
    /// Includes presets for common English, German, French, Spanish, Italian,
    /// Portuguese, Japanese, Chinese, Korean, and Arabic locales. Other locales
    /// fall back to en-US defaults.
    pub fn for_locale(locale: &str) -> Self {
        let normalized = locale.replace('_', "-").to_ascii_lowercase();
        let language = language_subtag(&normalized);
        match normalized.as_str() {
            "en-us" => Self {
                locale: locale.to_string(),
                decimal_separator: '.',
                thousands_separator: ',',
                date_format: "MM/dd/yyyy".to_string(),
            },
            "en-gb" | "en-au" | "en-nz" => Self {
                locale: locale.to_string(),
                decimal_separator: '.',
                thousands_separator: ',',
                date_format: "dd/MM/yyyy".to_string(),
            },
            _ if language == "de" => Self {
                locale: locale.to_string(),
                decimal_separator: ',',
                thousands_separator: '.',
                date_format: "dd.MM.yyyy".to_string(),
            },
            _ if language == "fr" => Self {
                locale: locale.to_string(),
                decimal_separator: ',',
                thousands_separator: '\u{202f}',
                date_format: "dd/MM/yyyy".to_string(),
            },
            _ if matches!(language, "es" | "it" | "pt") => Self {
                locale: locale.to_string(),
                decimal_separator: ',',
                thousands_separator: '.',
                date_format: "dd/MM/yyyy".to_string(),
            },
            _ if matches!(language, "ja" | "zh" | "ko") => Self {
                locale: locale.to_string(),
                decimal_separator: '.',
                thousands_separator: ',',
                date_format: "yyyy/MM/dd".to_string(),
            },
            _ if language == "ar" => Self {
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
        if value.is_nan() {
            return "NaN".to_string();
        }
        if value.is_infinite() {
            return if value.is_sign_negative() {
                "-∞"
            } else {
                "∞"
            }
            .to_string();
        }

        let decimal_places = decimal_places.min(MAX_DECIMAL_PLACES);
        let raw = format!("{:.*}", decimal_places, value.abs());
        let (integer_part, decimal_part) = raw.split_once('.').unwrap_or((&raw, ""));
        let rounded_is_zero = raw.bytes().all(|byte| matches!(byte, b'0' | b'.'));
        let formatted_integer =
            self.format_integer_digits(integer_part, value.is_sign_negative() && !rounded_is_zero);

        if decimal_places == 0 {
            return formatted_integer;
        }

        format!(
            "{}{}{}",
            formatted_integer, self.decimal_separator, decimal_part
        )
    }

    /// Formats an integer with locale-appropriate thousands separators.
    pub fn format_integer(&self, value: i64) -> String {
        self.format_integer_inner(value.unsigned_abs(), value < 0)
    }

    fn format_integer_inner(&self, abs_value: u64, negative: bool) -> String {
        let digits = abs_value.to_string();
        self.format_integer_digits(&digits, negative)
    }

    fn format_integer_digits(&self, digits: &str, negative: bool) -> String {
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
        let language = language_subtag(&self.locale);
        ["ar", "dv", "fa", "he", "ps", "ur", "yi"]
            .iter()
            .any(|candidate| language.eq_ignore_ascii_case(candidate))
    }

    /// Returns the locale identifier.
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Returns the locale's date pattern using `yyyy`, `MM`, and `dd` tokens.
    pub fn date_format(&self) -> &str {
        &self.date_format
    }

    /// Formats a validated Gregorian calendar date for this locale.
    pub fn format_date(&self, year: i32, month: u8, day: u8) -> anyhow::Result<String> {
        anyhow::ensure!((1..=9999).contains(&year), "year must be in 1..=9999");
        anyhow::ensure!((1..=12).contains(&month), "month must be in 1..=12");
        let max_day = days_in_month(year, month);
        anyhow::ensure!(
            (1..=max_day).contains(&day),
            "day must be in 1..={max_day} for {year:04}-{month:02}"
        );
        Ok(match self.date_format.as_str() {
            "dd.MM.yyyy" => format!("{day:02}.{month:02}.{year:04}"),
            "yyyy/MM/dd" => format!("{year:04}/{month:02}/{day:02}"),
            "dd/MM/yyyy" => format!("{day:02}/{month:02}/{year:04}"),
            _ => format!("{month:02}/{day:02}/{year:04}"),
        })
    }
}

fn language_subtag(locale: &str) -> &str {
    locale.split(['-', '_']).next().unwrap_or_default()
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 31,
    }
}

fn is_leap_year(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
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
        assert!(LocaleFormatter::for_locale("ur-PK").is_rtl());
        assert!(!LocaleFormatter::for_locale("art").is_rtl());
    }

    #[test]
    fn test_locale() {
        let fmt = LocaleFormatter::for_locale("de-DE");
        assert_eq!(fmt.locale(), "de-DE");
    }

    #[test]
    fn test_french_locale_uses_native_separators() {
        let fmt = LocaleFormatter::for_locale("fr-FR");
        assert_eq!(fmt.format_number(1234.56, 2), "1\u{202f}234,56");
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

    #[test]
    fn non_finite_large_and_excess_precision_values_are_bounded() {
        let fmt = LocaleFormatter::for_locale("en-US");
        assert_eq!(fmt.format_number(f64::NAN, 2), "NaN");
        assert_eq!(fmt.format_number(f64::INFINITY, 2), "∞");
        assert_eq!(fmt.format_number(f64::NEG_INFINITY, 2), "-∞");
        assert_eq!(fmt.format_number(1.25, usize::MAX).len(), 20);
        assert!(fmt.format_number(1e30, 0).contains(','));
    }

    #[test]
    fn formats_and_validates_locale_dates() {
        assert_eq!(
            LocaleFormatter::for_locale("en-US")
                .format_date(2024, 2, 29)
                .unwrap(),
            "02/29/2024"
        );
        assert_eq!(
            LocaleFormatter::for_locale("de_DE")
                .format_date(2025, 12, 31)
                .unwrap(),
            "31.12.2025"
        );
        assert!(
            LocaleFormatter::for_locale("en-US")
                .format_date(2023, 2, 29)
                .is_err()
        );
    }
}
