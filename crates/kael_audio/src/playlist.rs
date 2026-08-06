//! Playlist management.

use crate::AudioSource;

/// The repeat behavior used by a playlist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RepeatMode {
    /// Do not repeat when the playlist reaches the end.
    #[default]
    Off,
    /// Repeat the currently selected item.
    One,
    /// Repeat from the beginning after reaching the end.
    All,
}

/// A mutable playlist of audio sources.
#[derive(Debug, Clone)]
pub struct Playlist {
    items: Vec<AudioSource>,
    order: Vec<usize>,
    order_position: Option<usize>,
    repeat_mode: RepeatMode,
    shuffle_enabled: bool,
    shuffle_seed: u64,
}

impl Playlist {
    /// Creates an empty playlist.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            order: Vec::new(),
            order_position: None,
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            shuffle_seed: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Adds a new source to the playlist.
    pub fn add(&mut self, source: AudioSource) {
        let current = self.current_index();
        self.items.push(source);
        self.rebuild_order(current);
    }

    /// Returns the number of sources in the playlist.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether the playlist contains no sources.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Removes every source and clears the current selection.
    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.order_position = None;
    }

    /// Removes the source at `index` and returns it when present.
    pub fn remove(&mut self, index: usize) -> Option<AudioSource> {
        if index >= self.items.len() {
            return None;
        }

        let current = self.current_index();
        let removed = self.items.remove(index);
        let current = current.and_then(|current| {
            if self.items.is_empty() {
                None
            } else if current > index {
                Some(current - 1)
            } else if current == index {
                Some(index.min(self.items.len() - 1))
            } else {
                Some(current)
            }
        });
        self.rebuild_order(current);
        Some(removed)
    }

    /// Advances the playlist and returns the next source when one exists.
    pub fn next(&mut self) -> Option<&AudioSource> {
        if self.items.is_empty() {
            return None;
        }

        let next_position = match (self.order_position, self.repeat_mode) {
            (Some(position), RepeatMode::One) => position,
            (Some(position), _) if position + 1 < self.order.len() => position + 1,
            (_, RepeatMode::All) => 0,
            (None, _) => 0,
            _ => return None,
        };
        self.order_position = Some(next_position);
        self.current()
    }

    /// Moves to the previous source and returns it when one exists.
    pub fn previous(&mut self) -> Option<&AudioSource> {
        if self.items.is_empty() {
            return None;
        }

        let previous_position = match self.order_position {
            Some(position) if position > 0 => position - 1,
            Some(_) if self.repeat_mode == RepeatMode::All => self.order.len().saturating_sub(1),
            None => 0,
            _ => return None,
        };
        self.order_position = Some(previous_position);
        self.current()
    }

    /// Sets the repeat behavior.
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat_mode = mode;
    }

    /// Returns the configured repeat behavior.
    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat_mode
    }

    /// Enables or disables shuffle ordering.
    pub fn set_shuffle(&mut self, enabled: bool) {
        if self.shuffle_enabled == enabled {
            return;
        }

        self.shuffle_enabled = enabled;
        self.rebuild_order(self.current_index());
    }

    /// Enables shuffle with an application-provided seed and rebuilds the remaining order.
    pub fn shuffle_with_seed(&mut self, seed: u64) {
        self.shuffle_seed = seed;
        self.shuffle_enabled = true;
        self.rebuild_order(self.current_index());
    }

    /// Returns whether shuffle ordering is enabled.
    pub fn is_shuffle_enabled(&self) -> bool {
        self.shuffle_enabled
    }

    /// Returns the currently selected source.
    pub fn current(&self) -> Option<&AudioSource> {
        let order_position = self.order_position?;
        let item_index = *self.order.get(order_position)?;
        self.items.get(item_index)
    }
}

impl Default for Playlist {
    fn default() -> Self {
        Self::new()
    }
}

impl Playlist {
    fn current_index(&self) -> Option<usize> {
        self.order_position
            .and_then(|position| self.order.get(position).copied())
            .filter(|index| *index < self.items.len())
    }

    fn rebuild_order(&mut self, current: Option<usize>) {
        self.order = (0..self.items.len()).collect();

        if self.shuffle_enabled {
            deterministic_shuffle(&mut self.order, self.shuffle_seed ^ self.items.len() as u64);
        }

        self.order_position = if self.order.is_empty() {
            None
        } else if let Some(current) = current.filter(|index| *index < self.items.len()) {
            let current_position = self
                .order
                .iter()
                .position(|index| *index == current)
                .unwrap_or(0);
            self.order.rotate_left(current_position);
            Some(0)
        } else {
            Some(0)
        };
    }
}

fn deterministic_shuffle(order: &mut [usize], mut seed: u64) {
    if order.len() < 2 {
        return;
    }

    for index in (1..order.len()).rev() {
        seed ^= seed << 7;
        seed ^= seed >> 9;
        seed ^= seed << 8;
        let modulus = u64::try_from(index + 1).unwrap_or(u64::MAX);
        let swap_index = usize::try_from(seed % modulus).unwrap_or(0);
        order.swap(index, swap_index);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Playlist, RepeatMode};
    use crate::AudioSource;

    #[test]
    fn advances_forward_and_backward() {
        let mut playlist = Playlist::new();
        playlist.add(AudioSource::File("a.wav".into()));
        playlist.add(AudioSource::File("b.wav".into()));
        playlist.add(AudioSource::File("c.wav".into()));

        assert_eq!(playlist.current(), Some(&AudioSource::File("a.wav".into())));
        assert_eq!(playlist.next(), Some(&AudioSource::File("b.wav".into())));
        assert_eq!(
            playlist.previous(),
            Some(&AudioSource::File("a.wav".into()))
        );
        playlist.set_repeat(RepeatMode::All);
        playlist.previous();
        assert_eq!(playlist.current(), Some(&AudioSource::File("c.wav".into())));
    }

    #[test]
    fn shuffle_still_visits_all_items() {
        let mut playlist = Playlist::new();
        playlist.add(AudioSource::File("a.wav".into()));
        playlist.add(AudioSource::File("b.wav".into()));
        playlist.add(AudioSource::File("c.wav".into()));
        playlist.set_shuffle(true);

        let mut visited = BTreeSet::new();
        if let Some(current) = playlist.current() {
            visited.insert(format!("{:?}", current));
        }
        while let Some(next) = playlist.next() {
            visited.insert(format!("{:?}", next));
            if visited.len() == 3 {
                break;
            }
        }

        assert_eq!(visited.len(), 3);
    }

    #[test]
    fn removing_before_or_at_current_preserves_a_valid_selection() {
        let mut playlist = Playlist::new();
        for name in ["a.wav", "b.wav", "c.wav"] {
            playlist.add(AudioSource::File(name.into()));
        }
        playlist.next();
        assert_eq!(playlist.current(), Some(&AudioSource::File("b.wav".into())));

        playlist.remove(0);
        assert_eq!(playlist.current(), Some(&AudioSource::File("b.wav".into())));

        playlist.remove(0);
        assert_eq!(playlist.current(), Some(&AudioSource::File("c.wav".into())));
    }

    #[test]
    fn duplicate_sources_keep_the_selected_occurrence() {
        let duplicate = AudioSource::File("same.wav".into());
        let mut playlist = Playlist::new();
        playlist.add(duplicate.clone());
        playlist.add(duplicate);
        playlist.add(AudioSource::File("last.wav".into()));
        playlist.next();
        assert_eq!(playlist.current_index(), Some(1));
        playlist.set_shuffle(true);
        assert_eq!(playlist.current_index(), Some(1));
    }

    #[test]
    fn query_clear_and_seeded_shuffle_are_explicit() {
        let mut playlist = Playlist::new();
        assert!(playlist.is_empty());
        playlist.add(AudioSource::File("a.wav".into()));
        playlist.add(AudioSource::File("b.wav".into()));
        playlist.set_repeat(RepeatMode::One);
        playlist.shuffle_with_seed(42);

        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist.repeat_mode(), RepeatMode::One);
        assert!(playlist.is_shuffle_enabled());

        playlist.clear();
        assert!(playlist.is_empty());
        assert_eq!(playlist.current(), None);
    }
}
