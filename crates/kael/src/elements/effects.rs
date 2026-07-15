use crate::{
    AnyElement, App, Bounds, BoxShadow, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window,
    elements::cached::{
        CachedRequestLayoutState, cached_paint, cached_prepaint, cached_request_layout,
    },
    px,
};

#[track_caller]
/// Wraps a child subtree and applies CSS-style `filter` effects to it by rendering the
/// subtree to an offscreen tile and compositing it with a gaussian content blur and/or a
/// drop shadow that follows the subtree's actual silhouette.
///
/// The effect is a soft, frosted approximation rather than an exact CSS gaussian. The
/// shadow's spread radius (the `spread_radius` of the [`BoxShadow`]) is ignored; only its
/// `offset`, `color`, and `blur_radius` are used.
///
/// Place the layer inside a parent with a stable, explicit size (or give the child one) so
/// its offscreen tile bounds are well defined; a zero-or-flex-derived size can yield an empty
/// tile. Leave room around the child for a drop shadow's offset and blur, since the shadow is
/// clipped to the layer's content mask.
pub fn effect_layer(child: impl IntoElement) -> EffectLayer {
    EffectLayer {
        child: Some(child.into_any_element()),
        element_id: None,
        content_blur: px(0.),
        drop_shadow: None,
        source_location: core::panic::Location::caller(),
    }
}

/// A wrapper element that applies a content blur and/or drop shadow to a cached child subtree.
pub struct EffectLayer {
    child: Option<AnyElement>,
    element_id: Option<ElementId>,
    content_blur: Pixels,
    drop_shadow: Option<BoxShadow>,
    source_location: &'static core::panic::Location<'static>,
}

impl EffectLayer {
    /// Returns true when the effect layer still owns a child element.
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }

    /// Returns true when the caller supplied a stable explicit cache id.
    pub fn has_explicit_id(&self) -> bool {
        self.element_id.is_some()
    }

    /// Returns true when content blur is enabled.
    pub fn has_content_blur(&self) -> bool {
        self.content_blur > px(0.)
    }

    /// Coarse content blur class for content-safe diagnostics.
    pub fn content_blur_class(&self) -> &'static str {
        if self.content_blur <= px(0.) {
            "none"
        } else if self.content_blur <= px(4.) {
            "subtle"
        } else if self.content_blur <= px(12.) {
            "medium"
        } else {
            "heavy"
        }
    }

    /// Returns true when a drop shadow is configured.
    pub fn has_drop_shadow(&self) -> bool {
        self.drop_shadow.is_some()
    }

    /// Stable effect combination key.
    pub fn effect_key(&self) -> &'static str {
        match (self.has_content_blur(), self.has_drop_shadow()) {
            (false, false) => "none",
            (true, false) => "blur",
            (false, true) => "shadow",
            (true, true) => "blur_shadow",
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "effect_layer(has_child={}, explicit_id={}, effect={}, content_blur_class={}, has_drop_shadow={})",
            self.has_child(),
            self.has_explicit_id(),
            self.effect_key(),
            self.content_blur_class(),
            self.has_drop_shadow()
        )
    }

    /// Overrides the cache key used to preserve this subtree across frames.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.element_id = Some(id.into());
        self
    }

    /// Blurs the content of the wrapped subtree by the given radius, like `filter: blur()`.
    pub fn content_blur(mut self, radius: impl Into<Pixels>) -> Self {
        self.content_blur = radius.into();
        self
    }

    /// Draws a drop shadow of the wrapped subtree's silhouette, like `filter: drop-shadow()`.
    ///
    /// The shadow's `spread_radius` is ignored; only `offset`, `color`, and `blur_radius` apply.
    pub fn drop_shadow(mut self, shadow: BoxShadow) -> Self {
        self.drop_shadow = Some(shadow);
        self
    }
}

impl Element for EffectLayer {
    type RequestLayoutState = CachedRequestLayoutState;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        Some(
            self.element_id
                .clone()
                .unwrap_or_else(|| ElementId::CodeLocation(*self.source_location)),
        )
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        Some(self.source_location)
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        cached_request_layout(&mut self.child, window, cx)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        cached_prepaint(global_id.unwrap(), bounds, request_layout, window, cx)
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let content_blur = self.content_blur;
        let drop_shadow = self.drop_shadow.clone();
        cached_paint(
            global_id.unwrap(),
            bounds,
            prepaint,
            window,
            cx,
            move |window, surface| {
                window.paint_effect_surface(surface, content_blur, drop_shadow.as_ref());
            },
        );
    }
}

impl IntoElement for EffectLayer {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::effect_layer;
    use crate::{BoxShadow, ParentElement, div, hsla, point, px};

    #[test]
    fn effect_layer_defaults_to_no_effects() {
        let layer = effect_layer(div());
        assert_eq!(layer.content_blur, px(0.));
        assert!(layer.drop_shadow.is_none());
    }

    #[test]
    fn builder_methods_set_effects() {
        let shadow = BoxShadow {
            color: hsla(0., 0., 0., 0.5),
            offset: point(px(4.), px(6.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        };
        let layer = effect_layer(div().child("x"))
            .content_blur(px(6.))
            .drop_shadow(shadow);
        assert_eq!(layer.content_blur, px(6.));
        let stored = layer.drop_shadow.expect("shadow stored");
        assert_eq!(stored.blur_radius, px(8.));
        assert_eq!(stored.offset, point(px(4.), px(6.)));
    }

    #[test]
    fn effect_layer_summary_is_content_safe() {
        let shadow = BoxShadow {
            color: hsla(0., 0., 0., 0.5),
            offset: point(px(4.), px(6.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        };
        let layer = effect_layer(div().child("private frosted content"))
            .id("private-effect-cache")
            .content_blur(px(6.))
            .drop_shadow(shadow);

        assert!(layer.has_child());
        assert!(layer.has_explicit_id());
        assert!(layer.has_content_blur());
        assert_eq!(layer.content_blur_class(), "medium");
        assert!(layer.has_drop_shadow());
        assert_eq!(layer.effect_key(), "blur_shadow");

        let summary = layer.to_text();
        assert!(summary.contains("effect=blur_shadow"));
        assert!(summary.contains("content_blur_class=medium"));
        assert!(!summary.contains("private frosted content"));
        assert!(!summary.contains("private-effect-cache"));
        assert!(!summary.contains("6"));
        assert!(!summary.contains("8"));
    }
}

#[cfg(test)]
mod render_tests {
    use super::effect_layer;
    use crate::{
        BoxShadow, Context, IntoElement, POLYCHROME_SPRITE_KIND_CONTENT_BLURRED,
        POLYCHROME_SPRITE_KIND_CONTENT_SHADOW, POLYCHROME_SPRITE_KIND_PREMULTIPLIED, ParentElement,
        Render, Styled, TestAppContext, Window, div, hsla, point, px,
    };

    struct BlurView;
    impl Render for BlurView {
        fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().w_full().h_full().child(
                effect_layer(div().size(px(60.)).bg(crate::black()))
                    .id("blur")
                    .content_blur(px(6.)),
            )
        }
    }

    struct ShadowView;
    impl Render for ShadowView {
        fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().w_full().h_full().child(
                effect_layer(div().size(px(40.)).bg(crate::black()))
                    .id("shadow")
                    .drop_shadow(BoxShadow {
                        color: hsla(0., 0., 0., 0.6),
                        offset: point(px(8.), px(8.)),
                        blur_radius: px(6.),
                        spread_radius: px(0.),
                        inset: false,
                    }),
            )
        }
    }

    #[kael::test]
    fn content_blur_snapshots_then_composites_a_blurred_sprite(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| BlurView);

        cx.update(|window, _| {
            assert_eq!(window.rendered_scene().cached_surface_snapshots.len(), 1);
            assert_eq!(window.rendered_scene().polychrome_sprites.len(), 0);
        });

        cx.update(|window, cx| {
            window.draw(cx).clear();
            let sprites = &window.rendered_scene().polychrome_sprites;
            assert_eq!(sprites.len(), 1);
            assert_eq!(
                sprites[0].sprite_kind,
                POLYCHROME_SPRITE_KIND_CONTENT_BLURRED
            );
            assert!(sprites[0].blur_radius > 0.0);
        });
    }

    #[kael::test]
    fn drop_shadow_composites_shadow_then_premultiplied_content(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| ShadowView);

        cx.update(|window, _| {
            assert_eq!(window.rendered_scene().cached_surface_snapshots.len(), 1);
        });

        cx.update(|window, cx| {
            window.draw(cx).clear();
            let sprites = &window.rendered_scene().polychrome_sprites;
            assert_eq!(sprites.len(), 2);
            assert_eq!(
                sprites[0].sprite_kind,
                POLYCHROME_SPRITE_KIND_CONTENT_SHADOW
            );
            assert!(sprites[0].blur_radius > 0.0);
            assert!(sprites[0].color.a > 0.0);
            assert_eq!(sprites[1].sprite_kind, POLYCHROME_SPRITE_KIND_PREMULTIPLIED);
        });
    }
}
