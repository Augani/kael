#![deny(missing_docs)]

//! Internationalization support for the Kael framework.
//!
//! Provides string catalogs, pluralization rules, locale-aware formatting,
//! and a bundle manager for multi-locale applications.

mod bundle;
mod catalog;
mod locale;
mod plural;

pub use bundle::LocaleBundle;
pub use catalog::StringCatalog;
pub use locale::LocaleFormatter;
pub use plural::{PluralCategory, PluralRules};
