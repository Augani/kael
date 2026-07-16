//! Platform services icon scaffolding for Kael.

#![deny(missing_docs)]

/// Generated icon catalog scaffolding.
pub mod catalog;
/// Platform integration scaffolding for icon bridges.
pub mod platform;
/// Icon weight definitions.
pub mod weight;

pub use catalog::{IconMetadata, IconName, UnknownIconName};
pub use weight::IconWeight;

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

/// Returns the generated icon names currently available in the scaffold catalog.
pub fn generated_icon_names() -> &'static [&'static str] {
    catalog::generated_icon_names()
}

/// Returns the generated icon metadata currently available in the bundled catalog.
pub fn generated_icons() -> &'static [IconMetadata] {
    catalog::generated_icons()
}

/// Returns whether the active target currently has an implemented native icon bridge.
pub const fn has_native_bridge() -> bool {
    platform::has_native_bridge()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::{Icon, IconName, generated_icon_names, has_native_bridge};

    #[test]
    fn exposes_generated_icons() {
        assert!(generated_icon_names().contains(&"check"));
        let _ = has_native_bridge();
    }

    #[test]
    fn exposes_bundled_svg_sources() {
        let svg = Icon::new(IconName::Check).svg();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("currentColor"));
        assert_eq!(IconName::ChevronLeft.slug(), "chevron_left");
        assert_eq!(Icon::new(IconName::Check).stroke_width(), 1.5);
        assert_eq!(IconName::from_str("check").unwrap(), IconName::Check);
        assert_eq!(IconName::ALL.len(), generated_icon_names().len());
        assert_eq!(IconName::from_str("missing").unwrap_err().slug(), "missing");
    }
}
