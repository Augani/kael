use crate::{
    App, Bounds, Element, GlobalElementId, Hitbox, InspectorElementId, InteractiveElement,
    Interactivity, IntoElement, LayoutId, Pixels, Point, Radians, SharedString, Size,
    StyleRefinement, Styled, TransformationMatrix, Window, geometry::Negate as _, point, px,
    radians, size,
};
use util::ResultExt;

/// An SVG element.
pub struct Svg {
    interactivity: Interactivity,
    transformation: Option<Transformation>,
    path: Option<SharedString>,
}

/// Create a new SVG element.
#[track_caller]
pub fn svg() -> Svg {
    Svg {
        interactivity: Interactivity::new(),
        transformation: None,
        path: None,
    }
}

impl Svg {
    /// Set the path to the SVG file for this element.
    pub fn path(mut self, path: impl Into<SharedString>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Returns true when an SVG path is configured.
    pub fn has_path(&self) -> bool {
        self.path.is_some()
    }

    /// Byte length of the configured SVG path, without exposing the path.
    pub fn path_len_bytes(&self) -> usize {
        self.path.as_ref().map_or(0, |path| path.len())
    }

    /// Returns true when a transform is configured.
    pub fn has_transformation(&self) -> bool {
        self.transformation.is_some()
    }

    /// Stable transformation kind key.
    pub fn transformation_key(&self) -> &'static str {
        self.transformation
            .as_ref()
            .map_or("none", Transformation::kind_key)
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "svg(has_path={}, path_len_bytes={}, has_transformation={}, transformation={})",
            self.has_path(),
            self.path_len_bytes(),
            self.has_transformation(),
            self.transformation_key()
        )
    }

    /// Transform the SVG element with the given transformation.
    /// Note that this won't effect the hitbox or layout of the element, only the rendering.
    pub fn with_transformation(mut self, transformation: Transformation) -> Self {
        self.transformation = Some(transformation);
        self
    }
}

impl Element for Svg {
    type RequestLayoutState = ();
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<crate::ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.interactivity.source_location()
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout_id = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| window.request_layout(style, None, cx),
        );
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Hitbox> {
        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            bounds.size,
            window,
            cx,
            |_, _, hitbox, _, _| hitbox,
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Option<Hitbox>,
        window: &mut Window,
        cx: &mut App,
    ) where
        Self: Sized,
    {
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            |style, window, cx| {
                if let Some((path, color)) = self.path.as_ref().zip(style.text.color) {
                    let transformation = self
                        .transformation
                        .as_ref()
                        .map(|transformation| {
                            transformation.into_matrix(bounds.center(), window.scale_factor())
                        })
                        .unwrap_or_default();

                    window
                        .paint_svg(bounds, path.clone(), transformation, color, cx)
                        .log_err();
                }
            },
        )
    }
}

impl IntoElement for Svg {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for Svg {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for Svg {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

/// A transformation to apply to an SVG element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transformation {
    scale: Size<f32>,
    translate: Point<Pixels>,
    rotate: Radians,
}

impl Default for Transformation {
    fn default() -> Self {
        Self {
            scale: size(1.0, 1.0),
            translate: point(px(0.0), px(0.0)),
            rotate: radians(0.0),
        }
    }
}

impl Transformation {
    /// Returns true when the transformation changes scale.
    pub fn has_scale(&self) -> bool {
        self.scale != size(1.0, 1.0)
    }

    /// Returns true when the transformation changes translation.
    pub fn has_translation(&self) -> bool {
        self.translate != point(px(0.0), px(0.0))
    }

    /// Returns true when the transformation changes rotation.
    pub fn has_rotation(&self) -> bool {
        self.rotate != radians(0.0)
    }

    /// Stable transformation kind key.
    pub fn kind_key(&self) -> &'static str {
        match (
            self.has_scale(),
            self.has_translation(),
            self.has_rotation(),
        ) {
            (false, false, false) => "identity",
            (true, false, false) => "scale",
            (false, true, false) => "translate",
            (false, false, true) => "rotate",
            (true, true, false) => "scale_translate",
            (true, false, true) => "scale_rotate",
            (false, true, true) => "translate_rotate",
            (true, true, true) => "compound",
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "svg_transformation(kind={}, has_scale={}, has_translation={}, has_rotation={})",
            self.kind_key(),
            self.has_scale(),
            self.has_translation(),
            self.has_rotation()
        )
    }

    /// Create a new Transformation with the specified scale along each axis.
    pub fn scale(scale: Size<f32>) -> Self {
        Self {
            scale,
            translate: point(px(0.0), px(0.0)),
            rotate: radians(0.0),
        }
    }

    /// Create a new Transformation with the specified translation.
    pub fn translate(translate: Point<Pixels>) -> Self {
        Self {
            scale: size(1.0, 1.0),
            translate,
            rotate: radians(0.0),
        }
    }

    /// Create a new Transformation with the specified rotation in radians.
    pub fn rotate(rotate: impl Into<Radians>) -> Self {
        let rotate = rotate.into();
        Self {
            scale: size(1.0, 1.0),
            translate: point(px(0.0), px(0.0)),
            rotate,
        }
    }

    /// Update the scaling factor of this transformation.
    pub fn with_scaling(mut self, scale: Size<f32>) -> Self {
        self.scale = scale;
        self
    }

    /// Update the translation value of this transformation.
    pub fn with_translation(mut self, translate: Point<Pixels>) -> Self {
        self.translate = translate;
        self
    }

    /// Update the rotation angle of this transformation.
    pub fn with_rotation(mut self, rotate: impl Into<Radians>) -> Self {
        self.rotate = rotate.into();
        self
    }

    fn into_matrix(self, center: Point<Pixels>, scale_factor: f32) -> TransformationMatrix {
        //Note: if you read this as a sequence of matrix multiplications, start from the bottom
        TransformationMatrix::unit()
            .translate(center.scale(scale_factor) + self.translate.scale(scale_factor))
            .rotate(self.rotate)
            .scale(self.scale)
            .translate(center.scale(scale_factor).negate())
    }
}

#[cfg(test)]
mod tests {
    use super::{Transformation, svg};
    use crate::{point, px, radians, size};

    #[test]
    fn svg_summary_is_content_safe() {
        let transformation = Transformation::scale(size(2.0, 1.0))
            .with_translation(point(px(12.0), px(4.0)))
            .with_rotation(radians(0.25));
        assert_eq!(transformation.kind_key(), "compound");
        let transformation_summary = transformation.to_text();
        assert!(transformation_summary.contains("kind=compound"));
        assert!(!transformation_summary.contains("12"));
        assert!(!transformation_summary.contains("0.25"));

        let icon = svg()
            .path("/private/assets/secret-icon.svg")
            .with_transformation(transformation);
        assert!(icon.has_path());
        assert_eq!(
            icon.path_len_bytes(),
            "/private/assets/secret-icon.svg".len()
        );
        assert!(icon.has_transformation());
        assert_eq!(icon.transformation_key(), "compound");

        let summary = icon.to_text();
        assert!(summary.contains("svg(has_path=true"));
        assert!(summary.contains("transformation=compound"));
        assert!(!summary.contains("secret-icon"));
        assert!(!summary.contains("/private"));
    }
}
