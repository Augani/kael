//! # Theme file bridge
//!
//! Connects core's serializable, file-facing [`kael::Theme`] to `kael_ui`'s
//! runtime [`ThemeTokens`] vocabulary that components actually render from.
//!
//! Core owns the hot-reload file watcher: when a theme TOML/JSON file changes,
//! core re-parses it into a [`kael::Theme`], updates its own global, and then
//! notifies every subscriber registered through `App::observe_theme_files`.
//! This module registers one such subscriber that maps the loaded core theme
//! onto the current [`ThemeTokens`] and reinstalls them via [`install_theme`],
//! so file edits restyle live components through one observable pipeline.
//!
//! ## Mapping (core [`kael::Theme`] -> [`ThemeTokens`])
//!
//! The mapping starts from the *currently installed* tokens, so any token
//! field without a core source is preserved exactly. Mapped fields:
//!
//! | core field                  | token field(s)              |
//! |-----------------------------|-----------------------------|
//! | `colors.background`         | `background`                |
//! | `colors.foreground`         | `foreground`                |
//! | `colors.surface`            | `card`, `popover`           |
//! | `colors.primary`            | `primary`, `ring`           |
//! | `colors.accent`             | `accent`                    |
//! | `colors.muted`              | `muted`                     |
//! | `colors.border`             | `border`, `input`           |
//! | `colors.error`              | `destructive`               |
//! | `radii.sm/md/lg/xl`         | `radius_sm/md/lg/xl`        |
//! | `shadows.sm/md/lg`          | `shadow_sm/md/lg`           |
//! | `typography.ui_font_family` | `font_family`               |
//! | `typography.code_font_family`| `font_mono`                |
//!
//! Fields with no core source keep their existing token values:
//! `*_foreground` colors, `secondary`, `muted_foreground`, `accent_foreground`,
//! `shadow_xs`, `shadow_xl`, `ring_offset`, the spacing/duration/z-index
//! scales, transitions, and `radius`-less tokens. Core tokens without a token
//! target (`separator`, `selected_text`, `warning`, `success`, `radii.pill`,
//! the typographic sizes/weights) are intentionally dropped.

use kael::{App, Subscription, Theme as CoreTheme};
use smallvec::smallvec;

use super::{install_theme, Theme, ThemeTokens};

/// Maps a loaded core [`kael::Theme`] onto a base set of [`ThemeTokens`],
/// overlaying every field that has a core source and preserving the rest.
pub fn tokens_from_core_theme(core: &CoreTheme, base: &ThemeTokens) -> ThemeTokens {
    let mut tokens = base.clone();

    tokens.background = core.colors.background;
    tokens.foreground = core.colors.foreground;
    tokens.card = core.colors.surface;
    tokens.popover = core.colors.surface;
    tokens.primary = core.colors.primary;
    tokens.ring = core.colors.primary;
    tokens.accent = core.colors.accent;
    tokens.muted = core.colors.muted;
    tokens.border = core.colors.border;
    tokens.input = core.colors.border;
    tokens.destructive = core.colors.error;

    tokens.radius_sm = core.radii.sm;
    tokens.radius_md = core.radii.md;
    tokens.radius_lg = core.radii.lg;
    tokens.radius_xl = core.radii.xl;

    tokens.shadow_sm = smallvec![core.shadows.sm.clone()];
    tokens.shadow_md = smallvec![core.shadows.md.clone()];
    tokens.shadow_lg = smallvec![core.shadows.lg.clone()];

    tokens.font_family = core.typography.ui_font_family.clone();
    tokens.font_mono = core.typography.code_font_family.clone();

    tokens
}

/// Registers the bridge subscriber so that hot-reloaded theme files restyle
/// live components.
///
/// The subscriber reads the currently installed [`ThemeTokens`] as its base,
/// maps the freshly loaded core theme onto them via [`tokens_from_core_theme`],
/// and reinstalls the result through [`install_theme`] (which refreshes every
/// open window). The subscription is detached so it lives for the lifetime of
/// the app.
pub fn install_theme_file_bridge(cx: &mut App) {
    cx.observe_theme_files(|core, cx| {
        let base = Theme::try_get(cx)
            .map(|theme| theme.tokens.clone())
            .unwrap_or_else(ThemeTokens::dark);
        let tokens = tokens_from_core_theme(core, &base);
        install_theme(cx, Theme::custom(tokens));
    })
    .detach();
}

/// Registers the bridge subscriber and returns the [`Subscription`] handle for
/// callers that want to control its lifetime explicitly.
pub fn observe_theme_file_bridge(cx: &mut App) -> Subscription {
    cx.observe_theme_files(|core, cx| {
        let base = Theme::try_get(cx)
            .map(|theme| theme.tokens.clone())
            .unwrap_or_else(ThemeTokens::dark);
        let tokens = tokens_from_core_theme(core, &base);
        install_theme(cx, Theme::custom(tokens));
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kael::px;

    fn sample_core_toml() -> &'static str {
        r##"
[colors]
background = "#101820"
surface = "#19232d"
primary = "#2563eb"
accent = "#f59e0b"
muted = "#6b7280"
foreground = "#f9fafb"
border = "#334155"
error = "#dc2626"

[radii]
sm = 3.0
md = 7.0
lg = 11.0
xl = 15.0

[typography]
ui_font_family = "Inter"
code_font_family = "JetBrains Mono"
"##
    }

    #[test]
    fn maps_core_colors_onto_semantic_tokens() {
        let core = CoreTheme::from_toml_str(sample_core_toml()).unwrap();
        let tokens = tokens_from_core_theme(&core, &ThemeTokens::dark());

        assert_eq!(tokens.background, core.colors.background);
        assert_eq!(tokens.foreground, core.colors.foreground);
        assert_eq!(tokens.card, core.colors.surface);
        assert_eq!(tokens.popover, core.colors.surface);
        assert_eq!(tokens.primary, core.colors.primary);
        assert_eq!(tokens.ring, core.colors.primary);
        assert_eq!(tokens.accent, core.colors.accent);
        assert_eq!(tokens.muted, core.colors.muted);
        assert_eq!(tokens.border, core.colors.border);
        assert_eq!(tokens.input, core.colors.border);
        assert_eq!(tokens.destructive, core.colors.error);

        assert_eq!(tokens.radius_sm, px(3.0));
        assert_eq!(tokens.radius_md, px(7.0));
        assert_eq!(tokens.radius_lg, px(11.0));
        assert_eq!(tokens.radius_xl, px(15.0));

        assert_eq!(
            tokens.shadow_sm.as_slice(),
            std::slice::from_ref(&core.shadows.sm)
        );
        assert_eq!(
            tokens.shadow_md.as_slice(),
            std::slice::from_ref(&core.shadows.md)
        );
        assert_eq!(
            tokens.shadow_lg.as_slice(),
            std::slice::from_ref(&core.shadows.lg)
        );

        assert_eq!(tokens.font_family.as_ref(), "Inter");
        assert_eq!(tokens.font_mono.as_ref(), "JetBrains Mono");
    }

    #[test]
    fn partial_file_keeps_existing_tokens() {
        let base = ThemeTokens::dark();
        let core = CoreTheme::from_toml_str("[colors]\nprimary = \"#ff0000\"\n").unwrap();
        let tokens = tokens_from_core_theme(&core, &base);

        assert_eq!(tokens.primary, core.colors.primary);

        assert_eq!(tokens.secondary, base.secondary);
        assert_eq!(tokens.secondary_foreground, base.secondary_foreground);
        assert_eq!(tokens.primary_foreground, base.primary_foreground);
        assert_eq!(tokens.muted_foreground, base.muted_foreground);
        assert_eq!(tokens.accent_foreground, base.accent_foreground);
        assert_eq!(tokens.shadow_xs.as_slice(), base.shadow_xs.as_slice());
        assert_eq!(tokens.shadow_xl.as_slice(), base.shadow_xl.as_slice());
        assert_eq!(tokens.ring_offset, base.ring_offset);
        assert_eq!(tokens.spacing_4, base.spacing_4);
        assert_eq!(tokens.duration_normal, base.duration_normal);
        assert_eq!(tokens.z_modal, base.z_modal);
        assert_eq!(tokens.transition_base, base.transition_base);
    }

    #[test]
    fn round_trip_on_sample_toml_overlays_defaults() {
        let base = ThemeTokens::light();
        let core = CoreTheme::from_toml_str(sample_core_toml()).unwrap();
        let tokens = tokens_from_core_theme(&core, &base);

        assert_ne!(tokens.background, base.background);
        assert_eq!(tokens.background, core.colors.background);
        assert_eq!(tokens.primary, core.colors.primary);
        assert_eq!(tokens.font_family.as_ref(), "Inter");

        assert_eq!(tokens.spacing_4, base.spacing_4);
        assert_eq!(tokens.z_tooltip, base.z_tooltip);
    }
}
