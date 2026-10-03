//! Native atlas identities are never physical allocation slots. Reusing a slot
//! must not let a retained scene or delayed release identify a new texture.
use crate::{AtlasTextureId, AtlasTextureKind};
use collections::FxHashMap;
use std::sync::atomic::{AtomicU32, Ordering};

// Process-wide across native atlases, windows, and device generations. IDs are
// deliberately consumed even when a later driver allocation or admission fails.
static NEXT_TEXTURE_ID: AtomicU32 = AtomicU32::new(0);

fn allocate_id(counter: &AtomicU32, kind: AtlasTextureKind) -> anyhow::Result<AtlasTextureId> {
    let index = counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| anyhow::anyhow!("native atlas texture identity space exhausted"))?;
    Ok(AtlasTextureId { index, kind })
}

pub(super) fn allocate_native_atlas_texture_id(
    kind: AtlasTextureKind,
) -> anyhow::Result<AtlasTextureId> {
    allocate_id(&NEXT_TEXTURE_ID, kind)
}

pub(super) struct AtlasTextureList<T> {
    pub(super) textures: Vec<Option<T>>,
    free_list: Vec<usize>,
    slots_by_id: FxHashMap<u32, usize>,
}

impl<T> Default for AtlasTextureList<T> {
    fn default() -> Self {
        Self {
            textures: Vec::new(),
            free_list: Vec::new(),
            slots_by_id: FxHashMap::default(),
        }
    }
}

impl<T> AtlasTextureList<T> {
    pub(super) fn insert(&mut self, id: u32, texture: T) -> &mut T {
        let slot = self.free_list.pop().unwrap_or(self.textures.len());
        if slot == self.textures.len() {
            self.textures.push(Some(texture));
        } else {
            self.textures[slot] = Some(texture);
        }
        let previous = self.slots_by_id.insert(id, slot);
        debug_assert!(previous.is_none());
        self.textures[slot]
            .as_mut()
            .expect("new atlas slot is occupied")
    }

    pub(super) fn get(&self, id: u32) -> Option<&T> {
        self.textures.get(*self.slots_by_id.get(&id)?)?.as_ref()
    }

    pub(super) fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        self.textures.get_mut(*self.slots_by_id.get(&id)?)?.as_mut()
    }

    pub(super) fn remove(&mut self, id: u32) -> Option<T> {
        let slot = self.slots_by_id.remove(&id)?;
        let texture = self.textures[slot].take();
        self.free_list.push(slot);
        texture
    }

    #[allow(dead_code)]
    pub(super) fn drain(&mut self) -> std::vec::Drain<'_, Option<T>> {
        self.free_list.clear();
        self.slots_by_id.clear();
        self.textures.drain(..)
    }

    #[allow(dead_code)]
    pub(super) fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> {
        self.textures.iter_mut().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_atlas_identity_reuses_bounded_slots_without_aliasing_or_late_release() {
        let counter = AtomicU32::new(0);
        let mut list = AtlasTextureList::default();
        let first = allocate_id(&counter, AtlasTextureKind::Polychrome).unwrap();
        list.insert(first.index, 17);
        assert_eq!(list.remove(first.index), Some(17));
        for value in 0..10_000 {
            let id = allocate_id(&counter, AtlasTextureKind::Polychrome).unwrap();
            list.insert(id.index, value);
            assert!(list.get(first.index).is_none());
            assert!(list.get_mut(first.index).is_none());
            assert!(list.remove(first.index).is_none());
            assert_eq!(list.get(id.index), Some(&value));
            assert_eq!(list.textures.len(), 1);
            assert_eq!(list.slots_by_id.len(), 1);
            assert_eq!(list.remove(id.index), Some(value));
        }
        assert!(list.slots_by_id.is_empty());
        assert_eq!(list.free_list.len(), 1);
    }

    #[test]
    fn native_atlas_identity_survives_device_list_reset_and_failed_allocations() {
        let counter = AtomicU32::new(9);
        let old = allocate_id(&counter, AtlasTextureKind::Monochrome).unwrap();
        let mut list = AtlasTextureList::default();
        list.insert(old.index, 1);
        let failed = allocate_id(&counter, AtlasTextureKind::Monochrome).unwrap();
        // Reset driver-owned storage without resetting process identities.
        list = AtlasTextureList::default();
        let replacement = allocate_id(&counter, AtlasTextureKind::Monochrome).unwrap();
        list.insert(replacement.index, 2);
        assert!(replacement.index > failed.index && failed.index > old.index);
        assert!(list.get(old.index).is_none());
        assert!(list.get(failed.index).is_none());
        assert_eq!(list.get(replacement.index), Some(&2));
    }

    #[test]
    fn native_atlas_identity_exhaustion_fails_without_wrapping() {
        let counter = AtomicU32::new(u32::MAX - 1);
        assert_eq!(
            allocate_id(&counter, AtlasTextureKind::Monochrome)
                .unwrap()
                .index,
            u32::MAX - 1
        );
        for _ in 0..2 {
            assert!(allocate_id(&counter, AtlasTextureKind::Polychrome).is_err());
            assert_eq!(counter.load(Ordering::Relaxed), u32::MAX);
        }
    }
}
