use crate::{AssetSource, DevicePixels, IsZero, Result, SharedString, Size};
use resvg::tiny_skia::Pixmap;
use std::{
    borrow::Cow,
    hash::Hash,
    sync::{Arc, LazyLock},
};

/// When rendering SVGs, we render them at twice the size to get a higher-quality result.
pub const SMOOTH_SVG_SCALE_FACTOR: f32 = 2.;

#[derive(Clone, PartialEq, Hash, Eq)]
pub(crate) struct RenderSvgParams {
    pub(crate) path: SharedString,
    pub(crate) size: Size<DevicePixels>,
}

#[derive(Clone)]
pub struct SvgRenderer {
    asset_source: Arc<dyn AssetSource>,
    usvg_options: Arc<usvg::Options<'static>>,
}

pub enum SvgSize {
    Size(Size<DevicePixels>),
    ScaleFactor(f32),
}

impl SvgRenderer {
    pub fn new(asset_source: Arc<dyn AssetSource>) -> Self {
        static FONT_DB: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        });
        let default_font_resolver = usvg::FontResolver::default_font_selector();
        let font_resolver = Box::new(
            move |font: &usvg::Font, db: &mut Arc<usvg::fontdb::Database>| {
                if db.is_empty() {
                    *db = FONT_DB.clone();
                }
                default_font_resolver(font, db)
            },
        );
        let options = usvg::Options {
            font_resolver: usvg::FontResolver {
                select_font: font_resolver,
                select_fallback: usvg::FontResolver::default_fallback_selector(),
            },
            ..Default::default()
        };
        Self {
            asset_source,
            usvg_options: Arc::new(options),
        }
    }

    pub(crate) fn render(
        &self,
        params: &RenderSvgParams,
    ) -> Result<Option<(Size<DevicePixels>, Vec<u8>)>> {
        anyhow::ensure!(!params.size.is_zero(), "can't render at a zero size");

        // Load the application asset first so brand overrides always win, then
        // fall back to Kael's compact embedded icon catalog when enabled.
        let Some(bytes) = self.load_bytes(&params.path)? else {
            return Ok(None);
        };

        let pixmap = self.render_pixmap(&bytes, SvgSize::Size(params.size))?;

        // Convert the pixmap's pixels into an alpha mask.
        let size = Size::new(
            DevicePixels(pixmap.width() as i32),
            DevicePixels(pixmap.height() as i32),
        );
        let alpha_mask = pixmap
            .pixels()
            .iter()
            .map(|p| p.alpha())
            .collect::<Vec<_>>();
        Ok(Some((size, alpha_mask)))
    }

    fn load_bytes(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = self.asset_source.load(path)? {
            return Ok(Some(bytes));
        }

        #[cfg(feature = "icons")]
        if let Some(svg) = kael_icons::svg_for_path(path) {
            return Ok(Some(Cow::Borrowed(svg.as_bytes())));
        }

        Ok(None)
    }

    pub fn render_pixmap(&self, bytes: &[u8], size: SvgSize) -> Result<Pixmap, usvg::Error> {
        let tree = usvg::Tree::from_data(bytes, &self.usvg_options)?;
        let svg_size = tree.size();
        let scale = match size {
            SvgSize::Size(size) => size.width.0 as f32 / svg_size.width(),
            SvgSize::ScaleFactor(scale) => scale,
        };

        // Render the SVG to a pixmap with the specified width and height.
        let mut pixmap = resvg::tiny_skia::Pixmap::new(
            (svg_size.width() * scale) as u32,
            (svg_size.height() * scale) as u32,
        )
        .ok_or(usvg::Error::InvalidSize)?;

        let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);

        resvg::render(&tree, transform, &mut pixmap.as_mut());

        Ok(pixmap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OverrideAssets;

    impl AssetSource for OverrideAssets {
        fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
            Ok((path == "kael-icons/check.svg")
                .then_some(Cow::Borrowed(&b"application override"[..])))
        }

        fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn application_assets_take_precedence() {
        let renderer = SvgRenderer::new(Arc::new(OverrideAssets));
        let bytes = renderer
            .load_bytes("kael-icons/check.svg")
            .unwrap()
            .unwrap();
        assert_eq!(bytes.as_ref(), b"application override");
    }

    #[cfg(feature = "icons")]
    #[test]
    fn bundled_icons_fill_missing_virtual_assets() {
        let renderer = SvgRenderer::new(Arc::new(()));
        let bytes = renderer
            .load_bytes("kael-icons/circle-check.svg")
            .unwrap()
            .unwrap();
        assert!(bytes.starts_with(b"<svg"));

        let (size, alpha_mask) = renderer
            .render(&RenderSvgParams {
                path: "kael-icons/circle-check.svg".into(),
                size: Size::new(DevicePixels(24), DevicePixels(24)),
            })
            .unwrap()
            .unwrap();
        assert_eq!(size, Size::new(DevicePixels(24), DevicePixels(24)));
        assert!(alpha_mask.iter().any(|alpha| *alpha > 0));

        assert!(
            renderer
                .load_bytes("custom/circle-check.svg")
                .unwrap()
                .is_none()
        );
    }
}
