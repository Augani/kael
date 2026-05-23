//! Build-generated icon catalog.

include!(concat!(env!("OUT_DIR"), "/generated_icon_catalog.rs"));

/// Returns the generated icon names available in the scaffold catalog.
pub fn generated_icon_names() -> &'static [&'static str] {
    GENERATED_ICON_NAMES
}

/// Returns the generated icon metadata available in the bundled catalog.
pub fn generated_icons() -> &'static [IconMetadata] {
    GENERATED_ICONS
}

/// Returns the bundled SVG source for the given icon.
pub fn svg(name: IconName) -> &'static str {
    name.svg()
}