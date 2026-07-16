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

impl IconWeight {
    /// Returns a recommended stroke width for a 16×16 vector icon.
    pub const fn stroke_width(self) -> f32 {
        match self {
            Self::Thin => 1.0,
            Self::Light => 1.25,
            Self::Regular => 1.5,
            Self::Medium => 1.75,
            Self::SemiBold => 2.0,
            Self::Bold => 2.25,
            Self::Black => 2.5,
        }
    }
}
