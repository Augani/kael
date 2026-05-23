//! Icon weight definitions.

/// The supported icon weights in the scaffold catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconWeight {
    /// The thinnest supported icon weight.
    Thin,
    /// A light icon weight.
    Light,
    /// The default icon weight.
    Regular,
    /// A medium icon weight.
    Medium,
    /// A semi-bold icon weight.
    SemiBold,
    /// A bold icon weight.
    Bold,
    /// The heaviest icon weight.
    Black,
}
