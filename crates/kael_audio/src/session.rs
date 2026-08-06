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

/// A lightweight application audio-session state model.
///
/// This coordinates category, active state, routes, and interruptions within the application. It
/// does not claim ownership of an operating-system media session.
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

    /// Returns the current audio-session category.
    pub fn category(&self) -> AudioCategory {
        self.inner.lock().category
    }

    /// Activates or deactivates the audio session.
    pub fn set_active(&self, active: bool) {
        self.inner.lock().active = active;
    }

    /// Returns whether the application audio session is active.
    pub fn is_active(&self) -> bool {
        self.inner.lock().active
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
            let listener_id = allocate_listener_id(&mut state);
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
            let listener_id = allocate_listener_id(&mut state);
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

fn allocate_listener_id(state: &mut AudioSessionState) -> usize {
    let start = state.next_listener_id;
    let mut candidate = start;
    loop {
        if !state.route_listeners.contains_key(&candidate)
            && !state.interruption_listeners.contains_key(&candidate)
        {
            state.next_listener_id = candidate.wrapping_add(1);
            return candidate;
        }
        candidate = candidate.wrapping_add(1);
        assert!(
            candidate != start,
            "audio session listener id space exhausted"
        );
    }
}

impl Default for AudioSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_ids_wrap_without_replacing_callbacks() {
        let session = AudioSession::new();
        session.inner.lock().next_listener_id = usize::MAX;
        let max = session.on_route_change(|_| {});
        let zero = session.on_interruption(|_| {});
        let state = session.inner.lock();
        assert!(state.route_listeners.contains_key(&usize::MAX));
        assert!(state.interruption_listeners.contains_key(&0));
        drop(state);
        drop((max, zero));
    }

    #[test]
    fn listeners_are_notified_outside_the_state_lock() {
        let session = AudioSession::new();
        let callback_session = session.clone();
        let _subscription = session.on_route_change(move |_| {
            callback_session.set_category(AudioCategory::Ambient);
        });
        session.update_route(AudioRoute::Named("headphones".into()));
        assert_eq!(
            session.current_route(),
            AudioRoute::Named("headphones".into())
        );
        assert_eq!(session.inner.lock().category, AudioCategory::Ambient);
    }

    #[test]
    fn category_and_active_state_round_trip() {
        let session = AudioSession::new();
        session.set_category(AudioCategory::PlayAndRecord);
        session.set_active(true);

        assert_eq!(session.category(), AudioCategory::PlayAndRecord);
        assert!(session.is_active());
    }
}
