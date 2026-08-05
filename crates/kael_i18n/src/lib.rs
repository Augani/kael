#![deny(missing_docs)]

//! Internationalization support for the Kael framework.
//!
//! Provides bounded JSON string catalogs, integer pluralization rules, common
//! locale number/date conventions, and a bundle manager for multi-locale apps.
//! It intentionally stays lightweight rather than embedding full CLDR data.
//!
//! ```
//! use kael_i18n::{LocaleBundle, StringCatalog};
//!
//! let english = StringCatalog::from_json(
//!     "en-US",
//!     r#"{"welcome":"Welcome, {name}"}"#,
//! )?;
//! let mut bundle = LocaleBundle::new("en-US");
//! bundle.add_catalog(english);
//!
//! assert_eq!(
//!     bundle.translate_with_args("welcome", &[("name", "Ada")]),
//!     "Welcome, Ada"
//! );
//! # Ok::<(), anyhow::Error>(())
//! ```

mod bundle;
mod catalog;
mod locale;
mod plural;

pub use bundle::LocaleBundle;
pub use catalog::StringCatalog;
pub use locale::LocaleFormatter;
pub use plural::{PluralCategory, PluralRules};
