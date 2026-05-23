//! Audio-session state.

use std::{collections::BTreeMap, sync::Arc};

use parking_lot::Mutex;

use crate::player::Subscription;

type RouteListener = Arc<dyn Fn(AudioRoute) + Send + Sync + 'static>;
type InterruptionListener = Arc<dyn Fn(Interruption) + Send + Sync + 'static>;

/// The audio-session category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCategory {
    /// Playback-only audio.
    Playback,
    /// Recording-only audio.
    Record,
    /// Duplex playback and recording.
    PlayAndRecord,
    /// Ambient, non-exclusive audio.
    Ambient,
}

/// The currently selected audio route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioRoute {
    /// The default route chosen by the operating system.
    Default,
    /// A named output or input route.
    Named(String),
}

/// An audio interruption event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Interruption {
    /// Playback or recording was interrupted.
    Began,
    /// The interruption ended.
    Ended {
        /// Whether playback should resume automatically.
        should_resume: bool,
    },
}

/// A lightweight in-memory audio-session model.
#[derive(Clone)]
pub struct AudioSession {
    inner: Arc<Mutex<AudioSessionState>>,
}

struct AudioSessionState {
    category: AudioCategory,
    active: bool,
    route: AudioRoute,
    next_listener_id: usize,
    route_listeners: BTreeMap<usize, RouteListener>,
    interruption_listeners: BTreeMap<usize, InterruptionListener>,
}

impl AudioSession {
    /// Creates a new audio session.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AudioSessionState {
                category: AudioCategory::Playback,
                active: false,
                route: AudioRoute::Default,
                next_listener_id: 0,
                route_listeners: BTreeMap::new(),
                interruption_listeners: BTreeMap::new(),
            })),
        }
    }

    /// Sets the audio-session category.
    pub fn set_category(&self, category: AudioCategory) {
        self.inner.lock().category = category;
    }

    /// Activates or deactivates the audio session.
    pub fn set_active(&self, active: bool) -> anyhow::Result<()> {
        self.inner.lock().active = active;
        Ok(())
    }

    /// Returns the current audio route.
    pub fn current_route(&self) -> AudioRoute {
        self.inner.lock().route.clone()
    }

    /// Updates the current route and notifies listeners.
    pub fn update_route(&self, route: AudioRoute) {
        let listeners = {
            let mut state = self.inner.lock();
            state.route = route.clone();
            state.route_listeners.values().cloned().collect::<Vec<_>>()
        };

        for listener in listeners {
            listener(route.clone());
        }
    }

    /// Emits an interruption notification.
    pub fn emit_interruption(&self, interruption: Interruption) {
        let listeners = {
            let state = self.inner.lock();
            state
                .interruption_listeners
                .values()
                .cloned()
                .collect::<Vec<_>>()
        };

        for listener in listeners {
            listener(interruption.clone());
        }
    }

    /// Registers a listener for route changes.
    pub fn on_route_change(
        &self,
        callback: impl Fn(AudioRoute) + Send + Sync + 'static,
    ) -> Subscription {
        let state = self.inner.clone();
        let listener_id = {
            let mut state = state.lock();
            let listener_id = state.next_listener_id;
            state.next_listener_id += 1;
            state
                .route_listeners
                .insert(listener_id, Arc::new(callback));
            listener_id
        };

        Subscription::new(move || {
            state.lock().route_listeners.remove(&listener_id);
        })
    }

    /// Registers a listener for interruptions.
    pub fn on_interruption(
        &self,
        callback: impl Fn(Interruption) + Send + Sync + 'static,
    ) -> Subscription {
        let state = self.inner.clone();
        let listener_id = {
            let mut state = state.lock();
            let listener_id = state.next_listener_id;
            state.next_listener_id += 1;
            state
                .interruption_listeners
                .insert(listener_id, Arc::new(callback));
            listener_id
        };

        Subscription::new(move || {
            state.lock().interruption_listeners.remove(&listener_id);
        })
    }
}

impl Default for AudioSession {
    fn default() -> Self {
        Self::new()
    }
}
