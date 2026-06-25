//! Astryx design-language primitives shared across all Kael UI components.
//!
//! These helpers encode the parts of Facebook's open *Astryx* design system
//! that live below the semantic [`crate::theme::ThemeTokens`]: control sizing
//! (28 / 32 / 36 px element heights), the categorical hue palette used by
//! badges / tags / tokens, the 2px inset focus / validation rings, and the
//! hover / pressed overlay tints. Components read these so the whole library
//! shares one coherent look, while apps stay free to override any individual
//! value through a component's `StyleRefinement` or a custom theme.

use kael::*;

/// Astryx's signature snappy ease-out curve (`cubic-bezier(0.24, 1, 0.4, 1)`).
pub const ASTRYX_EASE: [f32; 4] = [0.24, 1.0, 0.4, 1.0];

/// The element-height ladder shared by buttons, inputs, selects, and other
/// interactive controls. Mirrors astryx `--size-element-{sm,md,lg}`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ControlSize {
    /// 28px tall — compact toolbars and dense forms.
    Sm,
    /// 32px tall — the default.
    #[default]
    Md,
    /// 36px tall — prominent, touch-friendly controls.
    Lg,
}

impl ControlSize {
    /// Control height in pixels (`--size-element-*`).
    pub fn height(self) -> Pixels {
        match self {
            ControlSize::Sm => px(28.0),
            ControlSize::Md => px(32.0),
            ControlSize::Lg => px(36.0),
        }
    }

    /// Horizontal padding for text-bearing controls.
    pub fn padding_x(self) -> Pixels {
        match self {
            ControlSize::Sm => px(10.0),
            ControlSize::Md => px(12.0),
            ControlSize::Lg => px(16.0),
        }
    }

    /// Gap between icon and label.
    pub fn gap(self) -> Pixels {
        match self {
            ControlSize::Sm => px(6.0),
            ControlSize::Md | ControlSize::Lg => px(8.0),
        }
    }

    /// Label font size.
    pub fn font_size(self) -> Pixels {
        match self {
            ControlSize::Sm => px(13.0),
            ControlSize::Md | ControlSize::Lg => px(14.0),
        }
    }

    /// Leading icon size.
    pub fn icon_size(self) -> Pixels {
        match self {
            ControlSize::Sm | ControlSize::Md => px(16.0),
            ControlSize::Lg => px(20.0),
        }
    }
}

/// A categorical hue from the astryx palette, used to color badges, tags,
/// tokens, status dots, and chart series with consistent, accessible stops.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Hue {
    Blue,
    Cyan,
    Gray,
    Green,
    Orange,
    Pink,
    Purple,
    Red,
    Teal,
    Yellow,
}

/// Resolved colors for one hue: a soft tinted `background`, a saturated
/// `border`, and a high-contrast `text` (also used for icons).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HueColors {
    pub background: Hsla,
    pub border: Hsla,
    pub text: Hsla,
}

impl Hue {
    /// All hues, in palette order — handy for showcases and pickers.
    pub const ALL: [Hue; 10] = [
        Hue::Blue,
        Hue::Cyan,
        Hue::Gray,
        Hue::Green,
        Hue::Orange,
        Hue::Pink,
        Hue::Purple,
        Hue::Red,
        Hue::Teal,
        Hue::Yellow,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Hue::Blue => "Blue",
            Hue::Cyan => "Cyan",
            Hue::Gray => "Gray",
            Hue::Green => "Green",
            Hue::Orange => "Orange",
            Hue::Pink => "Pink",
            Hue::Purple => "Purple",
            Hue::Red => "Red",
            Hue::Teal => "Teal",
            Hue::Yellow => "Yellow",
        }
    }

    /// Resolve this hue's `(background, border, text)` stops for the active mode.
    pub fn colors(self, dark: bool) -> HueColors {
        let (bg, border, text) = match (self, dark) {
            (Hue::Blue, false) => (0x0171E333, 0x0064E0FF, 0x042F97FF),
            (Hue::Blue, true) => (0x0171E333, 0x2694FEFF, 0xAFD7FFFF),
            (Hue::Cyan, false) => (0x03A7D733, 0x089DD0FF, 0x014975FF),
            (Hue::Cyan, true) => (0x03A7D733, 0x0171A4FF, 0xA1EEF9FF),
            (Hue::Gray, false) => (0x0A131722, 0x647685FF, 0x0A1317FF),
            (Hue::Gray, true) => (0x666A724C, 0x748695FF, 0xE7EAEDFF),
            (Hue::Green, false) => (0x24BB5E33, 0x0D8626FF, 0x09441FFF),
            (Hue::Green, true) => (0x24BB5E33, 0x0B991FFF, 0xA5F690FF),
            (Hue::Orange, false) => (0xF2790233, 0xEB6E00FF, 0x6B2203FF),
            (Hue::Orange, true) => (0xF2790233, 0xB34A01FF, 0xFDB876FF),
            (Hue::Pink, false) => (0xE638B333, 0xF351C0FF, 0x650053FF),
            (Hue::Pink, true) => (0xE638B333, 0xC02294FF, 0xFEADE3FF),
            (Hue::Purple, false) => (0x7952FF33, 0x9081FFFF, 0x3E0697FF),
            (Hue::Purple, true) => (0x7952FF33, 0x7340FEFF, 0xB3B0FEFF),
            (Hue::Red, false) => (0xE3193B33, 0xE3193BFF, 0x7B0210FF),
            (Hue::Red, true) => (0xE3193B33, 0xF5394FFF, 0xFFB2B8FF),
            (Hue::Teal, false) => (0x0DB7AF33, 0x08A3A3FF, 0x083943FF),
            (Hue::Teal, true) => (0x0DB7AF33, 0x08767DFF, 0x40DCCDFF),
            (Hue::Yellow, false) => (0xE2A40033, 0xC58600FF, 0x753F07FF),
            (Hue::Yellow, true) => (0xE2A40033, 0xB47700FF, 0xFBCE03FF),
        };
        HueColors {
            background: rgba(bg).into(),
            border: rgba(border).into(),
            text: rgba(text).into(),
        }
    }
}

/// Build astryx's 2px inset ring as a [`BoxShadow`] — the focus / selection
/// affordance used by inputs, selects, checkboxes, and switches. Sits *inside*
/// the element so it never shifts layout.
pub fn inset_ring(color: Hsla, width: Pixels) -> BoxShadow {
    BoxShadow {
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: width,
        inset: true,
        color,
    }
}

/// The standard 2px inset focus ring at astryx's `rgba(_, 0.5)` strength.
pub fn focus_ring(color: Hsla) -> BoxShadow {
    inset_ring(color.opacity(0.5), px(2.0))
}

/// A soft, low-strength outer focus ring (Tailwind-style) for controls that
/// read better with an outside halo than an inset stroke.
pub fn focus_ring_outer(color: Hsla) -> BoxShadow {
    BoxShadow {
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: px(3.0),
        inset: false,
        color: color.opacity(0.4),
    }
}

/// Hover overlay tint — a near-transparent ink wash layered over a control on
/// hover. `dark` picks the white (dark mode) vs black (light mode) ink.
pub fn overlay_hover(dark: bool) -> Hsla {
    if dark {
        hsla(0.0, 0.0, 1.0, 0.05)
    } else {
        hsla(0.0, 0.0, 0.0, 0.05)
    }
}

/// Pressed overlay tint — a slightly stronger ink wash for the active state.
pub fn overlay_pressed(dark: bool) -> Hsla {
    if dark {
        hsla(0.0, 0.0, 1.0, 0.10)
    } else {
        hsla(0.0, 0.0, 0.0, 0.10)
    }
}

/// Soft "muted" fill for a status/sentiment color (success / warning / error
/// banners and subtle badges) — the color at ~15% alpha.
pub fn status_muted(color: Hsla) -> Hsla {
    color.opacity(0.16)
}
