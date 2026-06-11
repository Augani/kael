use anyhow::{Context as _, Result, anyhow};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    App, BorrowAppContext, BoxShadow, FileWatchEvent, FileWatcher, FontWeight, Global, Hsla,
    Pixels, Rgba, SharedString, SubscriberSet, Subscription, Window, WindowAppearance, black,
    colors::{Colors, GlobalColors},
    point, px,
};

/// A callback invoked after a watched theme file reloads and the core
/// [`Theme`] global has been updated.
///
/// Registered through [`App::observe_theme_files`], every active subscriber is
/// notified with the freshly parsed [`Theme`] so downstream layers (such as
/// `kael_ui`'s token system) can bridge the file-facing theme onto their own
/// runtime representation.
pub type ThemeFileSubscriber = Box<dyn FnMut(&Theme, &mut App) + 'static>;

/// A full application theme that can be stored in GPUI global state.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Theme {
    /// The semantic color tokens used by the application.
    pub colors: ThemeColors,
    /// Typography tokens shared across UI primitives.
    pub typography: ThemeTypography,
    /// The canonical spacing scale for layout and component padding.
    pub spacing: ThemeSpacing,
    /// Shared corner radius tokens for surfaces and pills.
    pub radii: ThemeRadii,
    /// Reusable box-shadow tokens for depth and overlays.
    pub shadows: ThemeShadows,
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

impl Global for Theme {}

impl Theme {
    /// Installs the theme runtime and ensures theme-derived colors are available.
    pub fn init(cx: &mut App) {
        cx.update_default_global::<ThemeRuntime, _>(|runtime, cx| {
            if runtime.sync_subscription.is_none() {
                runtime.sync_subscription = Some(cx.observe_global::<Theme>(|cx| {
                    sync_theme_colors(cx);
                    cx.refresh_windows();
                }));
            }
        });

        if !cx.has_global::<Theme>() {
            cx.set_global(Self::default());
        }

        sync_theme_colors(cx);
    }

    /// Returns the default light theme.
    pub fn light() -> Self {
        Self::from_colors(Colors::light())
    }

    /// Returns the default dark theme.
    pub fn dark() -> Self {
        Self::from_colors(Colors::dark())
    }

    /// Returns the default theme matching a window's current appearance.
    pub fn for_appearance(window: &Window) -> Self {
        match window.appearance() {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
        }
    }

    /// Parses a theme from a JSON string.
    pub fn from_json_str(input: &str) -> Result<Self> {
        serde_json::from_str(input).context("failed to parse theme JSON")
    }

    /// Parses a theme from a TOML string.
    pub fn from_toml_str(input: &str) -> Result<Self> {
        toml::from_str(input).context("failed to parse theme TOML")
    }

    /// Loads a theme from a JSON or TOML file.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let input = fs::read_to_string(path)
            .with_context(|| format!("failed to read theme file {}", path.display()))?;

        match ThemeFileFormat::from_path(path) {
            Some(ThemeFileFormat::Json) => Self::from_json_str(&input),
            Some(ThemeFileFormat::Toml) => Self::from_toml_str(&input),
            None => {
                let (primary_label, primary, secondary_label, secondary) =
                    if input.trim_start().starts_with('{') {
                        (
                            "JSON",
                            Self::from_json_str(&input),
                            "TOML",
                            Self::from_toml_str(&input),
                        )
                    } else {
                        (
                            "TOML",
                            Self::from_toml_str(&input),
                            "JSON",
                            Self::from_json_str(&input),
                        )
                    };

                match primary {
                    Ok(theme) => Ok(theme),
                    Err(primary_error) => match secondary {
                        Ok(theme) => Ok(theme),
                        Err(secondary_error) => Err(anyhow!(
                            "failed to parse theme file {} as {} or {}: {primary_error:#}; {secondary_error:#}",
                            path.display(),
                            primary_label,
                            secondary_label,
                        )),
                    },
                }
            }
        }
    }

    fn from_colors(colors: Colors) -> Self {
        Self {
            colors: colors.into(),
            typography: ThemeTypography::default(),
            spacing: ThemeSpacing::default(),
            radii: ThemeRadii::default(),
            shadows: ThemeShadows::default(),
        }
    }
}

/// Semantic theme colors that can be mapped onto GPUI defaults and custom tokens.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ThemeColors {
    /// The primary application background color.
    pub background: Hsla,
    /// The default elevated or contained surface color.
    pub surface: Hsla,
    /// The primary interactive emphasis color.
    pub primary: Hsla,
    /// A secondary accent color for links or highlighted details.
    pub accent: Hsla,
    /// The muted or disabled foreground color.
    pub muted: Hsla,
    /// The default readable foreground color.
    pub foreground: Hsla,
    /// The shared border color.
    pub border: Hsla,
    /// The separator color for dividers.
    pub separator: Hsla,
    /// The foreground color used on selected or primary surfaces.
    pub selected_text: Hsla,
    /// The error status color.
    pub error: Hsla,
    /// The warning status color.
    pub warning: Hsla,
    /// The success status color.
    pub success: Hsla,
    /// Additional project-specific color tokens.
    pub custom: BTreeMap<SharedString, Hsla>,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Colors::default().into()
    }
}

/// Typography tokens shared across the application.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ThemeTypography {
    /// The font family used for general UI text.
    pub ui_font_family: SharedString,
    /// The default UI font weight.
    pub ui_font_weight: FontWeight,
    /// The default UI font size.
    pub ui_font_size: Pixels,
    /// The default UI line height.
    pub ui_line_height: Pixels,
    /// The font family used for code and monospace content.
    pub code_font_family: SharedString,
    /// The default code font size.
    pub code_font_size: Pixels,
}

impl Default for ThemeTypography {
    fn default() -> Self {
        Self {
            ui_font_family: ".SystemUIFont".into(),
            ui_font_weight: FontWeight::NORMAL,
            ui_font_size: px(14.),
            ui_line_height: px(20.),
            code_font_family: "Menlo".into(),
            code_font_size: px(13.),
        }
    }
}

/// Spacing tokens used for layout gaps and insets.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ThemeSpacing {
    /// The smallest spacing token.
    pub xs: Pixels,
    /// A small spacing token.
    pub sm: Pixels,
    /// The default spacing token.
    pub md: Pixels,
    /// A large spacing token.
    pub lg: Pixels,
    /// An extra-large spacing token.
    pub xl: Pixels,
    /// The largest spacing token.
    pub xxl: Pixels,
}

impl Default for ThemeSpacing {
    fn default() -> Self {
        Self {
            xs: px(4.),
            sm: px(8.),
            md: px(12.),
            lg: px(16.),
            xl: px(24.),
            xxl: px(32.),
        }
    }
}

/// Radius tokens used for corners and pills.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ThemeRadii {
    /// The smallest radius.
    pub sm: Pixels,
    /// A small radius.
    pub md: Pixels,
    /// The default radius.
    pub lg: Pixels,
    /// A large radius.
    pub xl: Pixels,
    /// A fully rounded radius for pills and badges.
    pub pill: Pixels,
}

impl Default for ThemeRadii {
    fn default() -> Self {
        Self {
            sm: px(4.),
            md: px(8.),
            lg: px(12.),
            xl: px(16.),
            pill: px(999.),
        }
    }
}

/// Box-shadow tokens used for elevation.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ThemeShadows {
    /// The smallest elevation shadow.
    pub sm: BoxShadow,
    /// The default elevation shadow.
    pub md: BoxShadow,
    /// The strongest elevation shadow.
    pub lg: BoxShadow,
}

impl Default for ThemeShadows {
    fn default() -> Self {
        Self {
            sm: BoxShadow {
                color: black().opacity(0.10),
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: false,
            },
            md: BoxShadow {
                color: black().opacity(0.14),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(-8.),
                inset: false,
            },
            lg: BoxShadow {
                color: black().opacity(0.18),
                offset: point(px(0.), px(16.)),
                blur_radius: px(40.),
                spread_radius: px(-12.),
                inset: false,
            },
        }
    }
}

impl From<Colors> for ThemeColors {
    fn from(colors: Colors) -> Self {
        Self {
            background: colors.background.into(),
            surface: colors.container.into(),
            primary: colors.selected.into(),
            accent: colors.selected.into(),
            muted: colors.disabled.into(),
            foreground: colors.text.into(),
            border: colors.border.into(),
            separator: colors.separator.into(),
            selected_text: colors.selected_text.into(),
            error: Rgba::try_from("#dc2626").unwrap().into(),
            warning: Rgba::try_from("#f59e0b").unwrap().into(),
            success: Rgba::try_from("#16a34a").unwrap().into(),
            custom: BTreeMap::default(),
        }
    }
}

impl From<&Theme> for Colors {
    fn from(theme: &Theme) -> Self {
        Self {
            text: theme.colors.foreground.into(),
            selected_text: theme.colors.selected_text.into(),
            background: theme.colors.background.into(),
            disabled: theme.colors.muted.into(),
            selected: theme.colors.primary.into(),
            border: theme.colors.border.into(),
            separator: theme.colors.separator.into(),
            container: theme.colors.surface.into(),
        }
    }
}

pub(crate) fn normalize_theme_path(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory while loading theme")?
            .join(path)
    };

    path.canonicalize()
        .with_context(|| format!("failed to resolve theme file {}", path.display()))
}

pub(crate) fn theme_file_event_matches_target(event: &FileWatchEvent, watched_path: &Path) -> bool {
    match event {
        FileWatchEvent::Created(path)
        | FileWatchEvent::Modified(path)
        | FileWatchEvent::Deleted(path) => same_theme_path(path, watched_path),
        FileWatchEvent::Renamed { from, to } => {
            same_theme_path(from, watched_path) || same_theme_path(to, watched_path)
        }
        FileWatchEvent::Error { path, .. } => same_theme_path(path, watched_path),
    }
}

pub(crate) fn retain_file_watcher(cx: &mut App, watcher: FileWatcher) {
    cx.update_default_global::<ThemeRuntime, _>(|runtime, _| {
        runtime.file_watchers.push(watcher);
    });
}

pub(crate) fn register_theme_file_subscriber(
    cx: &mut App,
    subscriber: ThemeFileSubscriber,
) -> Subscription {
    cx.update_default_global::<ThemeRuntime, _>(|runtime, _| {
        let (subscription, activate) = runtime.file_subscribers.insert((), subscriber);
        activate();
        subscription
    })
}

pub(crate) fn notify_theme_file_subscribers(cx: &mut App, theme: &Theme) {
    if !cx.has_global::<ThemeRuntime>() {
        return;
    }

    let subscribers = cx.global::<ThemeRuntime>().file_subscribers.clone();
    subscribers.retain(&(), |subscriber| {
        subscriber(theme, cx);
        true
    });
}

fn same_theme_path(candidate: &Path, watched_path: &Path) -> bool {
    if candidate == watched_path {
        return true;
    }

    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else if let Ok(current_dir) = std::env::current_dir() {
        current_dir.join(candidate)
    } else {
        return false;
    };

    absolute.canonicalize().unwrap_or(absolute) == watched_path
}

fn sync_theme_colors(cx: &mut App) {
    let colors = Colors::from(cx.global::<Theme>());
    cx.set_global(GlobalColors(Arc::new(colors)));
}

struct ThemeRuntime {
    sync_subscription: Option<Subscription>,
    file_watchers: Vec<FileWatcher>,
    file_subscribers: SubscriberSet<(), ThemeFileSubscriber>,
}

impl Default for ThemeRuntime {
    fn default() -> Self {
        Self {
            sync_subscription: None,
            file_watchers: Vec::new(),
            file_subscribers: SubscriberSet::new(),
        }
    }
}

impl Global for ThemeRuntime {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThemeFileFormat {
    Json,
    Toml,
}

impl ThemeFileFormat {
    fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some(extension) if extension.eq_ignore_ascii_case("json") => Some(Self::Json),
            Some(extension) if extension.eq_ignore_ascii_case("toml") => Some(Self::Toml),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use crate::colors::DefaultColors;

    static NEXT_TEMP_DIR_ID: AtomicU64 = AtomicU64::new(0);

    #[kael::test]
    fn setting_theme_updates_default_colors(cx: &mut crate::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(Theme::dark());
        });

        cx.read(|cx| {
            let colors = cx.default_colors();
            assert_eq!(colors.background, Colors::dark().background);
            assert_eq!(colors.text, Colors::dark().text);
            assert_eq!(colors.container, Colors::dark().container);
        });
    }

    #[kael::test]
    fn observe_theme_file_loads_initial_theme(cx: &mut crate::TestAppContext) {
        let directory = create_temp_theme_dir();
        let theme_path = directory.join("theme.toml");

        cx.on_quit({
            let directory = directory.clone();
            move || {
                let _ = fs::remove_dir_all(directory);
            }
        });

        fs::write(
            &theme_path,
            r##"
[colors]
background = "#101820"
surface = "#19232d"
primary = "#2563eb"
foreground = "#f9fafb"
    "##,
        )
        .unwrap();

        cx.update(|cx| {
            cx.observe_theme_file(&theme_path, |theme, cx| cx.set_global(theme))
                .unwrap();
        });

        let expected_theme = Theme::from_path(&theme_path).unwrap();
        let expected_colors = Colors::from(&expected_theme);

        cx.read_global::<Theme, _>(|theme, _| {
            assert_eq!(theme, &expected_theme);
        });

        cx.read(|cx| {
            let colors = cx.default_colors();
            assert_eq!(colors.background, expected_colors.background);
            assert_eq!(colors.selected, expected_colors.selected);
            assert_eq!(colors.text, expected_colors.text);
        });
    }

    #[kael::test]
    fn matching_theme_event_reloads_theme(cx: &mut crate::TestAppContext) {
        let directory = create_temp_theme_dir();
        let theme_path = directory.join("theme.toml");

        cx.on_quit({
            let directory = directory.clone();
            move || {
                let _ = fs::remove_dir_all(directory);
            }
        });

        fs::write(&theme_path, "[colors]\nbackground = \"#101820\"\n").unwrap();
        let watched_path = normalize_theme_path(&theme_path).unwrap();

        cx.update(|cx| Theme::init(cx));

        fs::write(
            &theme_path,
            "[colors]\nbackground = \"#1f2937\"\nprimary = \"#2563eb\"\nforeground = \"#f9fafb\"\n",
        )
        .unwrap();

        let expected_theme = Theme::from_path(&theme_path).unwrap();
        let expected_colors = Colors::from(&expected_theme);

        cx.update(|cx| {
            crate::app::handle_theme_file_event(
                cx,
                &FileWatchEvent::Modified(watched_path.clone()),
                &watched_path,
                &mut |theme, cx| cx.set_global(theme),
            )
            .unwrap();
        });

        cx.read_global::<Theme, _>(|theme, _| {
            assert_eq!(theme, &expected_theme);
        });

        cx.read(|cx| {
            let colors = cx.default_colors();
            assert_eq!(colors.background, expected_colors.background);
            assert_eq!(colors.selected, expected_colors.selected);
            assert_eq!(colors.text, expected_colors.text);
        });
    }

    #[kael::test]
    fn theme_file_subscriber_receives_reloaded_theme(cx: &mut crate::TestAppContext) {
        use std::{cell::RefCell, rc::Rc};

        let directory = create_temp_theme_dir();
        let theme_path = directory.join("theme.toml");

        cx.on_quit({
            let directory = directory.clone();
            move || {
                let _ = fs::remove_dir_all(directory);
            }
        });

        fs::write(&theme_path, "[colors]\nbackground = \"#101820\"\n").unwrap();
        let watched_path = normalize_theme_path(&theme_path).unwrap();

        let received: Rc<RefCell<Vec<Theme>>> = Rc::new(RefCell::new(Vec::new()));

        cx.update(|cx| {
            Theme::init(cx);
            let sink = received.clone();
            cx.observe_theme_files(move |theme, _| sink.borrow_mut().push(theme.clone()))
                .detach();
        });

        fs::write(
            &theme_path,
            "[colors]\nbackground = \"#1f2937\"\nprimary = \"#2563eb\"\nforeground = \"#f9fafb\"\n",
        )
        .unwrap();

        let expected_theme = Theme::from_path(&theme_path).unwrap();

        cx.update(|cx| {
            crate::app::handle_theme_file_event(
                cx,
                &FileWatchEvent::Modified(watched_path.clone()),
                &watched_path,
                &mut |theme, cx| cx.set_global(theme),
            )
            .unwrap();
        });

        let received = received.borrow();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0], expected_theme);
    }

    #[test]
    fn theme_file_event_matching_filters_unrelated_paths() {
        let directory = create_temp_theme_dir();
        let watched_path = directory.join("theme.toml");
        let other_path = directory.join("other.toml");

        fs::write(&watched_path, "[colors]\nbackground = \"#000000\"\n").unwrap();
        fs::write(&other_path, "[colors]\nbackground = \"#ffffff\"\n").unwrap();

        let watched_path = normalize_theme_path(&watched_path).unwrap();

        assert!(theme_file_event_matches_target(
            &FileWatchEvent::Modified(watched_path.clone()),
            &watched_path,
        ));
        assert!(theme_file_event_matches_target(
            &FileWatchEvent::Renamed {
                from: other_path,
                to: watched_path.clone(),
            },
            &watched_path,
        ));
        assert!(!theme_file_event_matches_target(
            &FileWatchEvent::Modified(directory.join("unrelated.toml")),
            &watched_path,
        ));

        let _ = fs::remove_dir_all(directory);
    }

    fn create_temp_theme_dir() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "kael-theme-test-{}-{}",
            std::process::id(),
            NEXT_TEMP_DIR_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }
}
