use crate::{AtlasTextureId, AtlasTile, Bounds, DevicePixels, Size, TileId, point};
use collections::FxHashMap;

/// Logical allocation identities for one packed page. The caller validates the
/// page identity before looking up or releasing a tile through this map.
///
/// Etagere's bucket generation is only eight bits. Its allocation IDs remain
/// private and never become scene identities, even after repeated bucket reuse.
/// Only current allocations occupy the map; released logical IDs are not kept.
#[derive(Default)]
pub(crate) struct AtlasTileAllocations {
    next_id: u64,
    active: FxHashMap<TileId, (etagere::AllocId, Bounds<DevicePixels>)>,
}

impl AtlasTileAllocations {
    pub(crate) fn allocate(
        &mut self,
        allocator: &mut etagere::BucketedAtlasAllocator,
        texture_id: AtlasTextureId,
        size: Size<DevicePixels>,
    ) -> Option<AtlasTile> {
        // Exhaustion rejects before touching the allocator. Successful issuance
        // is permanent, including allocations later rolled back by the backend.
        let tile_id = TileId(u32::try_from(self.next_id).ok()?);
        let allocation = allocator.allocate(etagere::size2(size.width.0, size.height.0))?;
        let bounds = Bounds {
            origin: point(
                DevicePixels(allocation.rectangle.min.x),
                DevicePixels(allocation.rectangle.min.y),
            ),
            size,
        };
        self.next_id += 1;
        self.active.insert(tile_id, (allocation.id, bounds));
        Some(AtlasTile {
            texture_id,
            tile_id,
            bounds,
            padding: 0,
        })
    }

    pub(crate) fn contains(&self, tile: &AtlasTile) -> bool {
        self.active
            .get(&tile.tile_id)
            .is_some_and(|(_, bounds)| *bounds == tile.bounds)
    }

    pub(crate) fn release(&mut self, tile: &AtlasTile) -> Option<etagere::AllocId> {
        if !self.contains(tile) {
            return None;
        }
        self.active.remove(&tile.tile_id).map(|(id, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AtlasTextureKind, size};

    fn page() -> AtlasTextureId {
        AtlasTextureId {
            kind: AtlasTextureKind::Polychrome,
            index: 7,
        }
    }

    #[test]
    fn retired_tiles_never_alias_after_bucket_generation_wraps() {
        let mut allocator = etagere::BucketedAtlasAllocator::new(etagere::size2(1024, 1024));
        let mut allocations = AtlasTileAllocations::default();
        let survivor = allocations
            .allocate(
                &mut allocator,
                page(),
                size(DevicePixels(8), DevicePixels(8)),
            )
            .unwrap();
        let requested = size(DevicePixels(1024), DevicePixels(512));
        let first = allocations
            .allocate(&mut allocator, page(), requested)
            .unwrap();
        let original_raw = allocations.release(&first).unwrap();
        allocator.deallocate(original_raw);
        let mut raw_identity_repeated = false;
        for _ in 0..512 {
            let current = allocations
                .allocate(&mut allocator, page(), requested)
                .unwrap();
            assert_ne!(current.tile_id, first.tile_id);
            assert!(!allocations.contains(&first));
            assert!(allocations.contains(&survivor));
            assert!(allocations.release(&first).is_none());
            assert_eq!(allocations.active.len(), 2);
            let raw = allocations.release(&current).unwrap();
            raw_identity_repeated |= raw == original_raw;
            allocator.deallocate(raw);
            assert_eq!(allocations.active.len(), 1);
        }
        assert!(
            raw_identity_repeated,
            "fixture must exercise actual etagere ID reuse"
        );
        allocator.deallocate(allocations.release(&survivor).unwrap());
        assert!(allocations.active.is_empty());
    }

    #[test]
    fn exact_bounds_and_one_time_release_protect_live_allocations() {
        let mut allocator = etagere::BucketedAtlasAllocator::new(etagere::size2(64, 64));
        let mut allocations = AtlasTileAllocations::default();
        let tile = allocations
            .allocate(
                &mut allocator,
                page(),
                size(DevicePixels(8), DevicePixels(8)),
            )
            .unwrap();
        let mut wrong = tile.clone();
        wrong.bounds.origin.x += DevicePixels(1);
        assert!(!allocations.contains(&wrong));
        assert!(allocations.release(&wrong).is_none());
        assert!(allocations.contains(&tile));
        allocator.deallocate(allocations.release(&tile).unwrap());
        assert!(allocations.release(&tile).is_none());
        assert!(!allocations.contains(&tile));
        assert!(allocations.active.is_empty());
    }

    #[test]
    fn checked_exhaustion_precedes_allocator_mutation_and_rollback_never_recycles() {
        let mut allocator = etagere::BucketedAtlasAllocator::new(etagere::size2(64, 64));
        let mut allocations = AtlasTileAllocations {
            next_id: u64::from(u32::MAX),
            ..Default::default()
        };
        let requested = size(DevicePixels(8), DevicePixels(8));
        let last = allocations
            .allocate(&mut allocator, page(), requested)
            .unwrap();
        assert_eq!(last.tile_id.0, u32::MAX);
        allocator.deallocate(allocations.release(&last).unwrap());
        let before = allocator.allocated_space();
        assert!(
            allocations
                .allocate(&mut allocator, page(), requested)
                .is_none()
        );
        assert_eq!(allocator.allocated_space(), before);
        assert!(allocations.active.is_empty());
    }
}
