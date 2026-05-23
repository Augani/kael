//! Spatial-audio bookkeeping.

use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;

use crate::Track;

/// A lightweight spatial-audio state container.
#[derive(Clone, Debug, Default)]
pub struct SpatialAudioPlayer {
    inner: Arc<Mutex<SpatialState>>,
}

#[derive(Debug)]
struct SpatialState {
    listener_position: [f32; 3],
    listener_forward: [f32; 3],
    listener_up: [f32; 3],
    source_positions: HashMap<u64, [f32; 3]>,
}

impl Default for SpatialState {
    fn default() -> Self {
        Self {
            listener_position: [0.0, 0.0, 0.0],
            listener_forward: [0.0, 0.0, -1.0],
            listener_up: [0.0, 1.0, 0.0],
            source_positions: HashMap::new(),
        }
    }
}

impl SpatialAudioPlayer {
    /// Sets the listener position.
    pub fn set_listener_position(&self, position: [f32; 3]) {
        self.inner.lock().listener_position = position;
    }

    /// Sets the listener orientation.
    pub fn set_listener_orientation(&self, forward: [f32; 3], up: [f32; 3]) {
        let mut state = self.inner.lock();
        state.listener_forward = forward;
        state.listener_up = up;
    }

    /// Sets the position for a track source.
    pub fn set_source_position(&self, track: &Track, position: [f32; 3]) {
        self.inner.lock().source_positions.insert(track.id, position);
    }

    /// Returns the current listener position.
    pub fn listener_position(&self) -> [f32; 3] {
        self.inner.lock().listener_position
    }

    /// Returns the current position for a track source.
    pub fn source_position(&self, track: &Track) -> Option<[f32; 3]> {
        self.inner.lock().source_positions.get(&track.id).copied()
    }
}