//! Compact, typed SVG icons for Kael applications and component libraries.
//!
//! The catalog contains the Lucide symbols used by Kael's ready-made UI plus
//! Kael's small set of core controls. Applications can use the typed catalog
//! directly or resolve the virtual [`ASSET_PREFIX`] paths emitted by `kael_ui`.

#![deny(missing_docs)]

/// Build-generated icon catalog.
pub mod catalog;
/// Icon weight definitions.
pub mod weight;

pub use catalog::{IconMetadata, IconName, UnknownIconName, svg_by_name};
pub use weight::IconWeight;

/// Virtual asset path prefix used for SVGs bundled in this crate.
pub const ASSET_PREFIX: &str = "kael-icons/";

/// A typed icon value with the chosen weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Icon {
    name: IconName,
    weight: IconWeight,
}

impl Icon {
    /// Creates a new icon with the default regular weight.
    pub const fn new(name: IconName) -> Self {
        Self {
            name,
            weight: IconWeight::Regular,
        }
    }

    /// Returns the typed icon name.
    pub const fn name(self) -> IconName {
        self.name
    }

    /// Returns the selected weight.
    pub const fn weight_value(self) -> IconWeight {
        self.weight
    }

    /// Returns a copy of this icon with a different weight.
    pub const fn weight(mut self, weight: IconWeight) -> Self {
        self.weight = weight;
        self
    }

    /// Returns the recommended stroke width for this icon's selected weight.
    pub const fn stroke_width(self) -> f32 {
        self.weight.stroke_width()
    }

    /// Returns the bundled SVG source for this icon.
    pub fn svg(self) -> &'static str {
        catalog::svg(self.name)
    }
}

/// Returns the generated icon names available in the bundled catalog.
pub fn generated_icon_names() -> &'static [&'static str] {
    catalog::generated_icon_names()
}

/// Returns the generated icon metadata currently available in the bundled catalog.
pub fn generated_icons() -> &'static [IconMetadata] {
    catalog::generated_icons()
}

/// Resolves a virtual `kael-icons/<name>.svg` path to bundled SVG source.
///
/// Both kebab-case and snake-case names are accepted. Other paths return
/// `None`, allowing an application asset source to handle them normally.
pub fn svg_for_path(path: &str) -> Option<&'static str> {
    let name = path.strip_prefix(ASSET_PREFIX)?.strip_suffix(".svg")?;
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return None;
    }
    svg_by_name(name)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::{ASSET_PREFIX, Icon, IconName, generated_icon_names, svg_for_path};

    #[test]
    fn exposes_generated_icons() {
        assert!(generated_icon_names().contains(&"check"));
        assert!(generated_icon_names().contains(&"search"));
        assert!(generated_icon_names().contains(&"volume_2"));
    }

    #[test]
    fn exposes_bundled_svg_sources() {
        let svg = Icon::new(IconName::Check).svg();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("currentColor"));
        assert_eq!(IconName::ChevronLeft.slug(), "chevron_left");
        assert_eq!(Icon::new(IconName::Check).stroke_width(), 1.5);
        assert_eq!(IconName::from_str("check").unwrap(), IconName::Check);
        assert_eq!(
            IconName::from_str("chevron-left").unwrap(),
            IconName::ChevronLeft
        );
        assert_eq!(IconName::ALL.len(), generated_icon_names().len());
        assert_eq!(IconName::from_str("missing").unwrap_err().slug(), "missing");
    }

    #[test]
    fn resolves_only_bundled_virtual_asset_paths() {
        let path = format!("{ASSET_PREFIX}circle-check.svg");
        assert!(svg_for_path(&path).unwrap().contains("<svg"));
        assert!(svg_for_path("assets/icons/circle-check.svg").is_none());
        assert!(svg_for_path("kael-icons/nested/check.svg").is_none());
        assert!(svg_for_path("kael-icons/check.png").is_none());
    }
}
