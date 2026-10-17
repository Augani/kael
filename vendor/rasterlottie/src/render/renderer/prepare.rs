use super::{
    super::assets::resolve_image_assets, ImageAssetResolver, LayerHierarchyCache,
    PreparedAnimation, Renderer, ShapeCaches, ShapePlanCache, StaticPathCache,
};
use crate::{
    Animation, RasterlottieError, SupportProfile, SupportReport, analyze_animation_with_profile,
    expression::resolve_supported_expressions,
};

impl Renderer {
    /// Creates a renderer that uses the provided support profile.
    #[must_use]
    pub const fn new(profile: SupportProfile) -> Self {
        Self { profile }
    }

    /// Creates a renderer configured with the default target-corpus support profile.
    #[must_use]
    pub const fn target_corpus() -> Self {
        Self::new(SupportProfile::target_corpus())
    }

    /// Analyzes an animation with this renderer's support profile.
    #[must_use]
    pub fn analyze(&self, animation: &Animation) -> SupportReport {
        analyze_animation_with_profile(animation, self.profile)
    }

    /// Preprocesses an animation for repeated rendering.
    ///
    /// # Errors
    ///
    /// Returns an error when the animation uses unsupported features or image
    /// asset preparation fails.
    pub fn prepare(&self, animation: &Animation) -> Result<PreparedAnimation, RasterlottieError> {
        let resolved = resolve_supported_expressions(animation);
        let report = analyze_animation_with_profile(&resolved, self.profile);
        if !report.is_supported() {
            return Err(RasterlottieError::UnsupportedFeatures { report });
        }

        let image_assets = resolve_image_assets(&resolved, None)?;
        let layer_hierarchy_cache = LayerHierarchyCache::from_animation(&resolved);
        let shape_plan_cache = ShapePlanCache::from_animation(&resolved);
        Ok(PreparedAnimation {
            renderer: *self,
            layer_hierarchy_cache,
            animation: resolved,
            image_assets,
            shape_plan_cache,
            static_path_cache: StaticPathCache::default(),
            timeline_sample_cache: super::super::sample_cache::TimelineSampleCache::default(),
        })
    }

    /// Preprocesses an animation while resolving external image assets through `resolver`.
    ///
    /// # Errors
    ///
    /// Returns an error when the animation uses unsupported features or the
    /// resolver cannot provide valid image bytes.
    pub fn prepare_with_resolver<R: ImageAssetResolver>(
        &self,
        animation: &Animation,
        resolver: &R,
    ) -> Result<PreparedAnimation, RasterlottieError> {
        let prepared_animation = resolve_supported_expressions(animation);
        let report = analyze_animation_with_profile(
            &prepared_animation,
            self.profile.with_external_image_assets(true),
        );
        if !report.is_supported() {
            return Err(RasterlottieError::UnsupportedFeatures { report });
        }

        let image_assets = resolve_image_assets(&prepared_animation, Some(resolver))?;
        let layer_hierarchy_cache = LayerHierarchyCache::from_animation(&prepared_animation);
        let shape_plan_cache = ShapePlanCache::from_animation(&prepared_animation);
        Ok(PreparedAnimation {
            renderer: *self,
            layer_hierarchy_cache,
            animation: prepared_animation,
            image_assets,
            shape_plan_cache,
            static_path_cache: StaticPathCache::default(),
            timeline_sample_cache: super::super::sample_cache::TimelineSampleCache::default(),
        })
    }
}

impl PreparedAnimation {
    /// Returns the preprocessed animation model.
    #[must_use]
    pub const fn animation(&self) -> &Animation {
        &self.animation
    }

    pub(super) const fn prepared_resources(&self) -> super::PreparedResources<'_> {
        super::PreparedResources {
            image_assets: &self.image_assets,
            shape_caches: ShapeCaches {
                static_paths: Some(&self.static_path_cache),
                plans: Some(&self.shape_plan_cache),
                timeline_samples: Some(&self.timeline_sample_cache),
            },
            layer_hierarchy_cache: Some(&self.layer_hierarchy_cache),
        }
    }
}
