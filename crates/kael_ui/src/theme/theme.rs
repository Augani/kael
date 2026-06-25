use kael::*;
use once_cell::sync::Lazy;
use std::sync::Arc;

use super::tokens::ThemeTokens;

/// Theme variants
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ThemeVariant {
    /// Astryx — Facebook's open design system, light (blue accent)
    Astryx,
    /// Astryx — dark (blue accent)
    AstryxDark,
    /// Astryx Neutral — grayscale spine, light
    AstryxNeutral,
    /// Astryx Neutral — grayscale spine, dark
    AstryxNeutralDark,
    /// Light theme
    Light,
    /// Dark theme
    Dark,
    /// Midnight Blue - Deep, calming dark blue tones
    MidnightBlue,
    /// Forest Grove - Natural greens with earthy accents
    ForestGrove,
    /// Sunset Amber - Warm oranges and deep purples
    SunsetAmber,
    /// Ocean Breeze - Cool blues and teals
    OceanBreeze,
    /// Dracula - Popular purple-based dark theme
    Dracula,
    /// Nord - Arctic, bluish color palette
    Nord,
    /// Monokai Pro - Vibrant syntax highlighting colors
    MonokaiPro,
    /// Tokyo Night - Modern dark theme with purple accents
    TokyoNight,
    /// Catppuccin Mocha - Pastel dark theme
    CatppuccinMocha,
    /// Rose Pine - Muted, natural tones
    RosePine,
    /// Coral Reef - Vibrant coral and turquoise
    CoralReef,
    /// Lavender Dreams - Soft purples and pastels
    LavenderDreams,
    /// Mint Fresh - Cool mint greens with clean whites
    MintFresh,
    /// Peachy Keen - Warm peach and orange tones
    PeachyKeen,
    /// Sky Blue - Bright blues inspired by clear skies
    SkyBlue,
    /// Cherry Blossom - Pink and magenta spring colors
    CherryBlossom,
    /// A user-defined theme built from custom [`ThemeTokens`]
    Custom,
}

impl ThemeVariant {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Astryx => "Astryx",
            Self::AstryxDark => "Astryx Dark",
            Self::AstryxNeutral => "Astryx Neutral",
            Self::AstryxNeutralDark => "Astryx Neutral Dark",
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::MidnightBlue => "Midnight Blue",
            Self::ForestGrove => "Forest Grove",
            Self::SunsetAmber => "Sunset Amber",
            Self::OceanBreeze => "Ocean Breeze",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::MonokaiPro => "Monokai Pro",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::RosePine => "Rose Pine",
            Self::CoralReef => "Coral Reef",
            Self::LavenderDreams => "Lavender Dreams",
            Self::MintFresh => "Mint Fresh",
            Self::PeachyKeen => "Peachy Keen",
            Self::SkyBlue => "Sky Blue",
            Self::CherryBlossom => "Cherry Blossom",
            Self::Custom => "Custom",
        }
    }
}

/// Kael-accessible theme wrapper
#[derive(Clone, Debug)]
pub struct Theme {
    pub variant: ThemeVariant,
    pub tokens: ThemeTokens,
}

impl Theme {
    /// Astryx — the default look: vivid blue accent on a tinted light canvas.
    pub fn astryx() -> Self {
        Self {
            variant: ThemeVariant::Astryx,
            tokens: ThemeTokens::astryx(),
        }
    }
    /// Astryx — dark mode.
    pub fn astryx_dark() -> Self {
        Self {
            variant: ThemeVariant::AstryxDark,
            tokens: ThemeTokens::astryx_dark(),
        }
    }
    /// Astryx Neutral — grayscale spine, light.
    pub fn astryx_neutral() -> Self {
        Self {
            variant: ThemeVariant::AstryxNeutral,
            tokens: ThemeTokens::astryx_neutral(),
        }
    }
    /// Astryx Neutral — grayscale spine, dark.
    pub fn astryx_neutral_dark() -> Self {
        Self {
            variant: ThemeVariant::AstryxNeutralDark,
            tokens: ThemeTokens::astryx_neutral_dark(),
        }
    }
    pub fn light() -> Self {
        Self {
            variant: ThemeVariant::Light,
            tokens: ThemeTokens::light(),
        }
    }
    pub fn dark() -> Self {
        Self {
            variant: ThemeVariant::Dark,
            tokens: ThemeTokens::dark(),
        }
    }
    pub fn midnight_blue() -> Self {
        Self {
            variant: ThemeVariant::MidnightBlue,
            tokens: ThemeTokens::midnight_blue(),
        }
    }
    pub fn forest_grove() -> Self {
        Self {
            variant: ThemeVariant::ForestGrove,
            tokens: ThemeTokens::forest_grove(),
        }
    }
    pub fn sunset_amber() -> Self {
        Self {
            variant: ThemeVariant::SunsetAmber,
            tokens: ThemeTokens::sunset_amber(),
        }
    }
    pub fn ocean_breeze() -> Self {
        Self {
            variant: ThemeVariant::OceanBreeze,
            tokens: ThemeTokens::ocean_breeze(),
        }
    }
    pub fn dracula() -> Self {
        Self {
            variant: ThemeVariant::Dracula,
            tokens: ThemeTokens::dracula(),
        }
    }
    pub fn nord() -> Self {
        Self {
            variant: ThemeVariant::Nord,
            tokens: ThemeTokens::nord(),
        }
    }
    pub fn monokai_pro() -> Self {
        Self {
            variant: ThemeVariant::MonokaiPro,
            tokens: ThemeTokens::monokai_pro(),
        }
    }
    pub fn tokyo_night() -> Self {
        Self {
            variant: ThemeVariant::TokyoNight,
            tokens: ThemeTokens::tokyo_night(),
        }
    }
    pub fn catppuccin_mocha() -> Self {
        Self {
            variant: ThemeVariant::CatppuccinMocha,
            tokens: ThemeTokens::catppuccin_mocha(),
        }
    }
    pub fn rose_pine() -> Self {
        Self {
            variant: ThemeVariant::RosePine,
            tokens: ThemeTokens::rose_pine(),
        }
    }
    pub fn coral_reef() -> Self {
        Self {
            variant: ThemeVariant::CoralReef,
            tokens: ThemeTokens::coral_reef(),
        }
    }
    pub fn lavender_dreams() -> Self {
        Self {
            variant: ThemeVariant::LavenderDreams,
            tokens: ThemeTokens::lavender_dreams(),
        }
    }
    pub fn mint_fresh() -> Self {
        Self {
            variant: ThemeVariant::MintFresh,
            tokens: ThemeTokens::mint_fresh(),
        }
    }
    pub fn peachy_keen() -> Self {
        Self {
            variant: ThemeVariant::PeachyKeen,
            tokens: ThemeTokens::peachy_keen(),
        }
    }
    pub fn sky_blue() -> Self {
        Self {
            variant: ThemeVariant::SkyBlue,
            tokens: ThemeTokens::sky_blue(),
        }
    }
    pub fn cherry_blossom() -> Self {
        Self {
            variant: ThemeVariant::CherryBlossom,
            tokens: ThemeTokens::cherry_blossom(),
        }
    }

    /// Build a theme from user-defined tokens, so an app can match its own brand.
    ///
    /// Start from any preset's tokens and override what you need with struct
    /// update syntax:
    ///
    /// ```rust,ignore
    /// let brand = Theme::custom(ThemeTokens {
    ///     primary: hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
    ///     radius_md: px(10.0),
    ///     ..ThemeTokens::dark()
    /// });
    /// install_theme(cx, brand);
    /// ```
    pub fn custom(tokens: ThemeTokens) -> Self {
        Self {
            variant: ThemeVariant::Custom,
            tokens,
        }
    }

    pub fn all() -> Vec<Theme> {
        vec![
            Self::astryx(),
            Self::astryx_dark(),
            Self::astryx_neutral(),
            Self::astryx_neutral_dark(),
            Self::dark(),
            Self::light(),
            Self::midnight_blue(),
            Self::forest_grove(),
            Self::sunset_amber(),
            Self::ocean_breeze(),
            Self::dracula(),
            Self::nord(),
            Self::monokai_pro(),
            Self::tokyo_night(),
            Self::catppuccin_mocha(),
            Self::rose_pine(),
            Self::coral_reef(),
            Self::lavender_dreams(),
            Self::mint_fresh(),
            Self::peachy_keen(),
            Self::sky_blue(),
            Self::cherry_blossom(),
        ]
    }
}

impl Global for Theme {}

impl Theme {
    /// Borrow the current theme from the app's global state.
    ///
    /// This is the zero-clone path: components should read tokens via
    /// `Theme::get(cx).tokens.*` rather than cloning the whole theme each
    /// render. Panics if no theme has been installed with [`install_theme`].
    pub fn get(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }

    /// Borrow the current theme from the app's global state.
    ///
    /// Alias of [`Theme::get`] for call sites that read tokens via
    /// `Theme::of(cx).tokens`.
    pub fn of(cx: &App) -> &Theme {
        Self::get(cx)
    }

    /// Borrow the current theme if one has been installed, without panicking.
    pub fn try_get(cx: &App) -> Option<&Theme> {
        cx.try_global::<Theme>()
    }
}

static THEME_STATE: Lazy<std::sync::Mutex<Arc<Theme>>> =
    Lazy::new(|| std::sync::Mutex::new(Arc::new(Theme::astryx())));

fn sync_theme_mirror(theme: &Theme) -> Arc<Theme> {
    let next = Arc::new(theme.clone());
    if let Ok(mut state) = THEME_STATE.lock() {
        *state = next.clone();
    }
    next
}

fn current_theme_mirror() -> Arc<Theme> {
    THEME_STATE
        .lock()
        .map(|guard| (*guard).clone())
        .unwrap_or_else(|_| Arc::new(Theme::dark()))
}

/// Install a theme globally for the app. Call early during app startup.
///
/// The theme is stored in the app's global state via `cx.set_global`, and read
/// back through [`Theme::get`] / [`Theme::of`]. Calling this again at runtime
/// switches the theme live: every open window is refreshed so components
/// re-read the new tokens on their next render.
pub fn install_theme(cx: &mut App, theme: Theme) {
    sync_theme_mirror(&theme);
    cx.set_global(theme);
    cx.refresh_windows();
}

/// Access a clone of the current theme.
///
/// **Deprecated path.** This clones the whole theme on every call and reads
/// from a process-global mirror rather than the app's global state. Prefer
/// [`Theme::get`] / [`Theme::of`], which borrow the theme from `cx` without
/// cloning. Retained as a shim so existing components keep compiling during
/// the migration to the [`Global`] path; the `#[deprecated]` attribute is
/// intentionally omitted so the in-progress callers do not emit warnings.
pub fn use_theme() -> Theme {
    (*current_theme_mirror()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn mirror_install_switch_and_custom_round_trip() {
        sync_theme_mirror(&Theme::light());
        assert_eq!(use_theme().variant, ThemeVariant::Light);

        let first = sync_theme_mirror(&Theme::nord());
        let mirrored = current_theme_mirror();
        assert!(Arc::ptr_eq(&first, &mirrored));
        assert_eq!(mirrored.variant, ThemeVariant::Nord);

        let second = sync_theme_mirror(&Theme::dracula());
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(current_theme_mirror().variant, ThemeVariant::Dracula);

        let tokens = ThemeTokens {
            radius_md: kael::px(13.0),
            ..ThemeTokens::dark()
        };
        sync_theme_mirror(&Theme::custom(tokens));
        let restored = use_theme();
        assert_eq!(restored.variant, ThemeVariant::Custom);
        assert_eq!(restored.tokens.radius_md, kael::px(13.0));
    }
}
