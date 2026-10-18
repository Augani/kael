use crate::{AtlasTextureId, AtlasTile, Scene, TileId};
use anyhow::{Result, ensure};
use collections::FxHashMap;

/// Hard limits applied before atlas rasterization, page allocation and upload.
///
/// Native packed atlases account resident GPU pages; browser atlases account
/// both CPU backing pages and their GPU mirrors. GTK additionally accounts its
/// immutable texture variants. Live or in-flight resources are never forced
/// out to satisfy a lower limit; new work is rejected until they can retire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasAdmissionLimits {
    /// Maximum resident atlas bytes, including backend-owned CPU mirrors.
    pub max_bytes: u64,
    /// Maximum live and deferred-retirement tile allocations.
    pub max_tiles: u32,
    /// Maximum resident texture pages (individual textures on GTK).
    pub max_pages: u32,
}

impl Default for AtlasAdmissionLimits {
    fn default() -> Self {
        Self {
            max_bytes: 256 * 1024 * 1024,
            max_tiles: 65_536,
            max_pages: 1_024,
        }
    }
}

impl AtlasAdmissionLimits {
    /// Validate nonzero byte, tile and page limits without changing an atlas.
    pub fn validate(self) -> Result<Self> {
        ensure!(
            self.max_bytes > 0,
            "atlas byte admission limit must be nonzero"
        );
        ensure!(
            self.max_tiles > 0,
            "atlas tile admission limit must be nonzero"
        );
        ensure!(
            self.max_pages > 0,
            "atlas page admission limit must be nonzero"
        );
        Ok(self)
    }
}

type TileIdentity = (AtlasTextureId, TileId);

/// Scene clocks advance only after the backend's safe progress gate.
/// Metal uses three drawables and retains four submission frames; Blade waits
/// for the preceding scene fence. DirectX/Web uploads share the ordered scene
/// command stream, and GTK presentations retain immutable texture references.
/// This clock is not a universal GPU completion counter.
#[derive(Default)]
pub(crate) struct AtlasPolicy {
    pub(crate) limits: AtlasAdmissionLimits,
    explicit_limits: bool,
    frame: u64,
    last_used: FxHashMap<TileIdentity, u64>,
    retired: Vec<AtlasTile>,
    retirement_frames_remaining: u8,
}

impl AtlasPolicy {
    #[cfg(any(test, target_os = "windows"))]
    pub(crate) fn reset_runtime(&mut self) {
        *self = Self {
            limits: self.limits,
            explicit_limits: self.explicit_limits,
            ..Default::default()
        };
    }
    pub(crate) fn set_hard_limits(&mut self, limits: AtlasAdmissionLimits) {
        self.limits = limits;
        self.explicit_limits = true;
    }

    pub(crate) fn set_soft_budget(&mut self, bytes: Option<u64>) {
        if !self.explicit_limits {
            self.limits.max_bytes = AtlasAdmissionLimits::default()
                .max_bytes
                .max(bytes.unwrap_or(0));
        }
    }

    pub(crate) fn check_tile(&self, raster_bytes: usize) -> Result<()> {
        ensure!(
            self.last_used.len() < self.limits.max_tiles as usize,
            "atlas tile admission limit reached; live or in-flight tiles are retained"
        );
        ensure!(
            raster_bytes as u64 <= self.limits.max_bytes,
            "atlas raster exceeds byte admission limit"
        );
        Ok(())
    }

    pub(crate) fn check_page(
        &self,
        resident_bytes: u64,
        pages: usize,
        added_bytes: u64,
    ) -> Result<()> {
        ensure!(
            pages < self.limits.max_pages as usize,
            "atlas page admission limit reached"
        );
        ensure!(
            resident_bytes
                .checked_add(added_bytes)
                .is_some_and(|bytes| bytes <= self.limits.max_bytes),
            "atlas resident byte admission limit reached; live or in-flight pages are retained"
        );
        Ok(())
    }

    pub(crate) fn touch(&mut self, tile: &AtlasTile) {
        self.last_used
            .insert((tile.texture_id, tile.tile_id), self.frame);
    }

    pub(crate) fn mark_scene_used(&mut self, scene: &Scene) {
        for tile in scene.atlas_tiles() {
            let identity = (tile.texture_id, tile.tile_id);
            if let Some(frame) = self.last_used.get_mut(&identity) {
                *frame = self.frame;
            }
        }
    }

    pub(crate) fn last_used(&self, tile: &AtlasTile) -> u64 {
        self.last_used
            .get(&(tile.texture_id, tile.tile_id))
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn guard(&self, keep_frames: u64) -> u64 {
        self.frame.saturating_sub(keep_frames.saturating_sub(1))
    }

    pub(crate) fn forget(&mut self, tile: &AtlasTile) {
        self.last_used.remove(&(tile.texture_id, tile.tile_id));
    }

    pub(crate) fn retire(&mut self, tile: AtlasTile) {
        // Cache ownership can end after a submission; preserve its region until
        // four successful scene frames have passed, including replayed uses.
        self.touch(&tile);
        self.retired.push(tile);
        self.retirement_frames_remaining = 4;
    }

    pub(crate) fn advance(&mut self) -> Vec<AtlasTile> {
        self.retirement_frames_remaining = self.retirement_frames_remaining.saturating_sub(1);
        if self.frame == u64::MAX {
            self.frame = 1;
            self.last_used.values_mut().for_each(|frame| *frame = 0);
        } else {
            self.frame += 1;
        }
        let guard = self.guard(4);
        let mut ready = Vec::new();
        self.retired.retain(|tile| {
            if self
                .last_used
                .get(&(tile.texture_id, tile.tile_id))
                .copied()
                .unwrap_or(0)
                < guard
            {
                ready.push(tile.clone());
                false
            } else {
                true
            }
        });
        for tile in &ready {
            self.forget(tile);
        }
        ready
    }

    pub(crate) fn needs_retirement_frames(&self) -> bool {
        self.retirement_frames_remaining > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AtlasTextureKind, Bounds, CachedSurfaceSnapshot, DevicePixels, point, size};

    fn tile(index: u32) -> AtlasTile {
        AtlasTile {
            texture_id: AtlasTextureId {
                index,
                kind: AtlasTextureKind::Polychrome,
            },
            tile_id: TileId(index),
            padding: 0,
            bounds: Bounds {
                origin: point(DevicePixels(0), DevicePixels(0)),
                size: size(DevicePixels(1), DevicePixels(1)),
            },
        }
    }

    #[test]
    fn hard_admission_checks_overflow_counts_and_explicit_limits() {
        let mut policy = AtlasPolicy::default();
        let limits = AtlasAdmissionLimits {
            max_bytes: 16,
            max_tiles: 1,
            max_pages: 1,
        };
        policy.set_hard_limits(limits);
        policy.set_soft_budget(Some(u64::MAX));
        assert_eq!(policy.limits, limits);
        policy.reset_runtime();
        assert_eq!(
            policy.limits, limits,
            "device restoration retains explicit admission policy"
        );
        assert!(policy.check_page(u64::MAX, 0, 1).is_err());
        assert!(policy.check_page(0, 1, 4).is_err());
        assert!(policy.check_page(12, 0, 4).is_ok());
        assert!(policy.check_tile(17).is_err());
        policy.touch(&tile(0));
        assert!(policy.check_tile(1).is_err());
        assert!(
            AtlasAdmissionLimits {
                max_bytes: 0,
                ..limits
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn retired_regions_wait_four_successful_frames_and_replay_refreshes_use() {
        let mut policy = AtlasPolicy::default();
        let tile = tile(0);
        policy.touch(&tile);
        policy.retire(tile.clone());
        for _ in 0..3 {
            assert!(policy.advance().is_empty());
        }
        // Without clock advancement (including a failed fence), ownership stays.
        assert!(policy.check_tile(0).is_ok());
        let mut scene = Scene::default();
        scene.cached_surface_snapshots.push(CachedSurfaceSnapshot {
            paint_operations: 0..0,
            source_bounds: tile.bounds,
            target: tile.clone(),
        });
        policy.mark_scene_used(&scene);
        assert!(policy.advance().is_empty());
        assert!(
            !policy.needs_retirement_frames(),
            "replayed owners must not force endless animation"
        );
        for _ in 0..2 {
            assert!(policy.advance().is_empty());
        }
        assert_eq!(policy.advance(), vec![tile]);
        assert!(policy.last_used.is_empty());
    }

    #[test]
    fn clock_rollover_preserves_retired_and_live_regions_conservatively() {
        let mut policy = AtlasPolicy::default();
        policy.frame = u64::MAX;
        let tile = tile(1);
        policy.touch(&tile);
        policy.retire(tile.clone());
        assert!(policy.advance().is_empty());
        assert_eq!(policy.frame, 1);
        assert!(policy.advance().is_empty());
        assert!(policy.advance().is_empty());
        assert_eq!(policy.advance(), vec![tile]);
    }
}
