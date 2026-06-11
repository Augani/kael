#![allow(dead_code)]

use collections::{HashMap, HashSet};
use std::fmt;

use crate::Keystroke;

pub fn keystroke_to_trigger(keystroke: &Keystroke) -> Option<String> {
    let key = key_name_to_xkb_keysym(&keystroke.key)?;

    let mut parts: Vec<&str> = Vec::with_capacity(5);
    if keystroke.modifiers.control {
        parts.push("CTRL");
    }
    if keystroke.modifiers.alt {
        parts.push("ALT");
    }
    if keystroke.modifiers.shift {
        parts.push("SHIFT");
    }
    if keystroke.modifiers.platform {
        parts.push("LOGO");
    }

    let mut trigger = String::new();
    for part in &parts {
        trigger.push_str(part);
        trigger.push('+');
    }
    trigger.push_str(key);
    Some(trigger)
}

fn key_name_to_xkb_keysym(key: &str) -> Option<&'static str> {
    match key.to_lowercase().as_str() {
        "a" => Some("a"),
        "b" => Some("b"),
        "c" => Some("c"),
        "d" => Some("d"),
        "e" => Some("e"),
        "f" => Some("f"),
        "g" => Some("g"),
        "h" => Some("h"),
        "i" => Some("i"),
        "j" => Some("j"),
        "k" => Some("k"),
        "l" => Some("l"),
        "m" => Some("m"),
        "n" => Some("n"),
        "o" => Some("o"),
        "p" => Some("p"),
        "q" => Some("q"),
        "r" => Some("r"),
        "s" => Some("s"),
        "t" => Some("t"),
        "u" => Some("u"),
        "v" => Some("v"),
        "w" => Some("w"),
        "x" => Some("x"),
        "y" => Some("y"),
        "z" => Some("z"),
        "0" => Some("0"),
        "1" => Some("1"),
        "2" => Some("2"),
        "3" => Some("3"),
        "4" => Some("4"),
        "5" => Some("5"),
        "6" => Some("6"),
        "7" => Some("7"),
        "8" => Some("8"),
        "9" => Some("9"),
        "space" => Some("space"),
        "enter" | "return" => Some("Return"),
        "tab" => Some("Tab"),
        "escape" => Some("Escape"),
        "backspace" => Some("BackSpace"),
        "delete" => Some("Delete"),
        "insert" => Some("Insert"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pageup" => Some("Page_Up"),
        "pagedown" => Some("Page_Down"),
        "left" => Some("Left"),
        "up" => Some("Up"),
        "right" => Some("Right"),
        "down" => Some("Down"),
        "f1" => Some("F1"),
        "f2" => Some("F2"),
        "f3" => Some("F3"),
        "f4" => Some("F4"),
        "f5" => Some("F5"),
        "f6" => Some("F6"),
        "f7" => Some("F7"),
        "f8" => Some("F8"),
        "f9" => Some("F9"),
        "f10" => Some("F10"),
        "f11" => Some("F11"),
        "f12" => Some("F12"),
        "-" => Some("minus"),
        "=" => Some("equal"),
        "[" => Some("bracketleft"),
        "]" => Some("bracketright"),
        "\\" => Some("backslash"),
        ";" => Some("semicolon"),
        "'" => Some("apostrophe"),
        "`" => Some("grave"),
        "," => Some("comma"),
        "." => Some("period"),
        "/" => Some("slash"),
        _ => None,
    }
}

pub fn shortcut_id(id: u32) -> String {
    id.to_string()
}

pub fn parse_shortcut_id(shortcut_id: &str) -> Option<u32> {
    shortcut_id.parse::<u32>().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutRequest {
    pub id: String,
    pub description: String,
    pub preferred_trigger: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundShortcut {
    pub id: String,
    pub trigger_description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalError {
    Unavailable(String),
    Bind(String),
}

impl fmt::Display for PortalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortalError::Unavailable(reason) => {
                write!(f, "GlobalShortcuts portal unavailable: {reason}")
            }
            PortalError::Bind(reason) => {
                write!(f, "GlobalShortcuts portal bind failed: {reason}")
            }
        }
    }
}

impl std::error::Error for PortalError {}

pub trait ShortcutBinder {
    fn bind(
        &mut self,
        requests: &[ShortcutRequest],
    ) -> std::result::Result<Vec<BoundShortcut>, PortalError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Pending,
    Active,
    Failed,
}

pub struct PortalShortcutState {
    state: SessionState,
    pending: HashMap<u32, Keystroke>,
    descriptions: HashMap<u32, String>,
    bound: HashMap<u32, String>,
    last_error: Option<PortalError>,
}

impl PortalShortcutState {
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
            pending: HashMap::default(),
            descriptions: HashMap::default(),
            bound: HashMap::default(),
            last_error: None,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn last_error(&self) -> Option<&PortalError> {
        self.last_error.as_ref()
    }

    pub fn register(&mut self, id: u32, keystroke: &Keystroke, description: impl Into<String>) {
        self.pending.insert(id, keystroke.clone());
        self.descriptions.insert(id, description.into());
        if self.state == SessionState::Active {
            self.state = SessionState::Pending;
        }
    }

    pub fn unregister(&mut self, id: u32) {
        self.pending.remove(&id);
        self.descriptions.remove(&id);
        self.bound.remove(&id);
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn pending_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.pending.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub fn bound_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.bound.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub fn trigger_description(&self, id: u32) -> Option<&str> {
        self.bound.get(&id).map(String::as_str)
    }

    pub fn requests(&self) -> Vec<ShortcutRequest> {
        self.pending_ids()
            .into_iter()
            .filter_map(|id| {
                let keystroke = self.pending.get(&id)?;
                let description = self
                    .descriptions
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| format!("Shortcut {id}"));
                Some(ShortcutRequest {
                    id: shortcut_id(id),
                    description,
                    preferred_trigger: keystroke_to_trigger(keystroke),
                })
            })
            .collect()
    }

    pub fn drive<B: ShortcutBinder>(
        &mut self,
        binder: &mut B,
    ) -> std::result::Result<Vec<BoundShortcut>, PortalError> {
        if self.pending.is_empty() {
            self.state = SessionState::Idle;
            return Ok(Vec::new());
        }

        self.state = SessionState::Pending;
        let requests = self.requests();
        match binder.bind(&requests) {
            Ok(bound) => {
                self.apply_bind_result(&bound);
                self.state = SessionState::Active;
                self.last_error = None;
                Ok(bound)
            }
            Err(err) => {
                self.state = SessionState::Failed;
                self.last_error = Some(err.clone());
                Err(err)
            }
        }
    }

    pub fn apply_bind_result(&mut self, bound: &[BoundShortcut]) {
        let mut confirmed = HashSet::default();
        for shortcut in bound {
            if let Some(id) = parse_shortcut_id(&shortcut.id) {
                self.bound.insert(id, shortcut.trigger_description.clone());
                confirmed.insert(id);
            }
        }
        self.bound.retain(|id, _| confirmed.contains(id));
    }

    pub fn on_activated(&self, shortcut_id: &str) -> Option<u32> {
        let id = parse_shortcut_id(shortcut_id)?;
        if self.bound.contains_key(&id) {
            Some(id)
        } else {
            None
        }
    }

    pub fn on_deactivated(&self, shortcut_id: &str) -> Option<u32> {
        self.on_activated(shortcut_id)
    }
}

impl Default for PortalShortcutState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Modifiers;

    fn keystroke(key: &str, control: bool, alt: bool, shift: bool, platform: bool) -> Keystroke {
        Keystroke {
            modifiers: Modifiers {
                control,
                alt,
                shift,
                platform,
                function: false,
            },
            key: key.to_string(),
            key_char: None,
        }
    }

    #[test]
    fn trigger_plain_key() {
        let ks = keystroke("a", false, false, false, false);
        assert_eq!(keystroke_to_trigger(&ks).as_deref(), Some("a"));
    }

    #[test]
    fn trigger_ctrl_shift_order() {
        let ks = keystroke("k", true, false, true, false);
        assert_eq!(keystroke_to_trigger(&ks).as_deref(), Some("CTRL+SHIFT+k"));
    }

    #[test]
    fn trigger_all_modifiers_canonical_order() {
        let ks = keystroke("f", true, true, true, true);
        assert_eq!(
            keystroke_to_trigger(&ks).as_deref(),
            Some("CTRL+ALT+SHIFT+LOGO+f")
        );
    }

    #[test]
    fn trigger_platform_maps_to_logo() {
        let ks = keystroke("space", false, false, false, true);
        assert_eq!(keystroke_to_trigger(&ks).as_deref(), Some("LOGO+space"));
    }

    #[test]
    fn trigger_named_keys_use_xkb_names() {
        let ret = keystroke("enter", true, false, false, false);
        assert_eq!(keystroke_to_trigger(&ret).as_deref(), Some("CTRL+Return"));
        let pgup = keystroke("pageup", false, true, false, false);
        assert_eq!(keystroke_to_trigger(&pgup).as_deref(), Some("ALT+Page_Up"));
        let left = keystroke("left", false, false, false, true);
        assert_eq!(keystroke_to_trigger(&left).as_deref(), Some("LOGO+Left"));
    }

    #[test]
    fn trigger_unknown_key_returns_none() {
        let ks = keystroke("nonexistent", true, false, false, false);
        assert_eq!(keystroke_to_trigger(&ks), None);
    }

    #[test]
    fn shortcut_id_roundtrips() {
        assert_eq!(shortcut_id(42), "42");
        assert_eq!(parse_shortcut_id("42"), Some(42));
        assert_eq!(parse_shortcut_id("not-a-number"), None);
    }

    enum MockMode {
        Echo,
        Altered(Vec<BoundShortcut>),
        Failing(PortalError),
    }

    struct MockBinder {
        mode: MockMode,
        last_requests: Vec<ShortcutRequest>,
        calls: usize,
    }

    impl MockBinder {
        fn echo() -> Self {
            Self {
                mode: MockMode::Echo,
                last_requests: Vec::new(),
                calls: 0,
            }
        }

        fn failing(err: PortalError) -> Self {
            Self {
                mode: MockMode::Failing(err),
                last_requests: Vec::new(),
                calls: 0,
            }
        }

        fn altered(bound: Vec<BoundShortcut>) -> Self {
            Self {
                mode: MockMode::Altered(bound),
                last_requests: Vec::new(),
                calls: 0,
            }
        }
    }

    impl ShortcutBinder for MockBinder {
        fn bind(
            &mut self,
            requests: &[ShortcutRequest],
        ) -> std::result::Result<Vec<BoundShortcut>, PortalError> {
            self.calls += 1;
            self.last_requests = requests.to_vec();
            match &self.mode {
                MockMode::Echo => Ok(requests
                    .iter()
                    .map(|req| BoundShortcut {
                        id: req.id.clone(),
                        trigger_description: req
                            .preferred_trigger
                            .clone()
                            .unwrap_or_else(|| req.id.clone()),
                    })
                    .collect()),
                MockMode::Altered(bound) => Ok(bound.clone()),
                MockMode::Failing(err) => Err(err.clone()),
            }
        }
    }

    #[test]
    fn state_machine_initial_idle() {
        let state = PortalShortcutState::new();
        assert_eq!(state.state(), SessionState::Idle);
        assert!(state.is_empty());
    }

    #[test]
    fn drive_with_no_pending_is_idle() {
        let mut state = PortalShortcutState::new();
        let mut binder = MockBinder::echo();
        let bound = state.drive(&mut binder).unwrap();
        assert!(bound.is_empty());
        assert_eq!(state.state(), SessionState::Idle);
        assert_eq!(binder.calls, 0);
    }

    #[test]
    fn register_then_drive_becomes_active() {
        let mut state = PortalShortcutState::new();
        state.register(1, &keystroke("k", true, false, true, false), "Open palette");
        let mut binder = MockBinder::echo();
        let bound = state.drive(&mut binder).unwrap();
        assert_eq!(state.state(), SessionState::Active);
        assert_eq!(bound.len(), 1);
        assert_eq!(state.bound_ids(), vec![1]);
        assert_eq!(binder.last_requests.len(), 1);
        assert_eq!(binder.last_requests[0].id, "1");
        assert_eq!(
            binder.last_requests[0].preferred_trigger.as_deref(),
            Some("CTRL+SHIFT+k")
        );
        assert_eq!(binder.last_requests[0].description, "Open palette");
    }

    #[test]
    fn portal_unavailable_sets_failed_and_surfaces_error() {
        let mut state = PortalShortcutState::new();
        state.register(1, &keystroke("a", false, false, false, false), "Test");
        let mut binder = MockBinder::failing(PortalError::Unavailable("no portal".into()));
        let err = state.drive(&mut binder).unwrap_err();
        assert_eq!(err, PortalError::Unavailable("no portal".into()));
        assert_eq!(state.state(), SessionState::Failed);
        assert!(state.last_error().is_some());
        assert!(state.bound_ids().is_empty());
    }

    #[test]
    fn consent_can_alter_trigger() {
        let mut state = PortalShortcutState::new();
        state.register(7, &keystroke("k", true, false, true, false), "Palette");
        let mut binder = MockBinder::altered(vec![BoundShortcut {
            id: "7".to_string(),
            trigger_description: "Super+P".to_string(),
        }]);
        state.drive(&mut binder).unwrap();
        assert_eq!(state.trigger_description(7).as_deref(), Some("Super+P"));
    }

    #[test]
    fn consent_can_drop_a_shortcut() {
        let mut state = PortalShortcutState::new();
        state.register(1, &keystroke("a", true, false, false, false), "One");
        state.register(2, &keystroke("b", true, false, false, false), "Two");
        let mut binder = MockBinder::altered(vec![BoundShortcut {
            id: "1".to_string(),
            trigger_description: "Ctrl+A".to_string(),
        }]);
        state.drive(&mut binder).unwrap();
        assert_eq!(state.bound_ids(), vec![1]);
        assert_eq!(state.trigger_description(2), None);
    }

    #[test]
    fn activated_maps_bound_shortcut_to_id() {
        let mut state = PortalShortcutState::new();
        state.register(5, &keystroke("a", true, false, false, false), "Five");
        let mut binder = MockBinder::echo();
        state.drive(&mut binder).unwrap();
        assert_eq!(state.on_activated("5"), Some(5));
        assert_eq!(state.on_deactivated("5"), Some(5));
    }

    #[test]
    fn activated_unknown_shortcut_is_ignored() {
        let mut state = PortalShortcutState::new();
        state.register(5, &keystroke("a", true, false, false, false), "Five");
        let mut binder = MockBinder::echo();
        state.drive(&mut binder).unwrap();
        assert_eq!(state.on_activated("999"), None);
        assert_eq!(state.on_activated("garbage"), None);
    }

    #[test]
    fn unregister_removes_from_bound() {
        let mut state = PortalShortcutState::new();
        state.register(5, &keystroke("a", true, false, false, false), "Five");
        let mut binder = MockBinder::echo();
        state.drive(&mut binder).unwrap();
        assert_eq!(state.bound_ids(), vec![5]);
        state.unregister(5);
        assert_eq!(state.on_activated("5"), None);
        assert!(state.bound_ids().is_empty());
    }

    #[test]
    fn register_after_active_marks_pending() {
        let mut state = PortalShortcutState::new();
        state.register(1, &keystroke("a", true, false, false, false), "One");
        let mut binder = MockBinder::echo();
        state.drive(&mut binder).unwrap();
        assert_eq!(state.state(), SessionState::Active);
        state.register(2, &keystroke("b", true, false, false, false), "Two");
        assert_eq!(state.state(), SessionState::Pending);
    }

    #[test]
    fn requests_are_sorted_and_complete() {
        let mut state = PortalShortcutState::new();
        state.register(3, &keystroke("c", true, false, false, false), "C");
        state.register(1, &keystroke("a", true, false, false, false), "A");
        state.register(2, &keystroke("b", true, false, false, false), "B");
        let requests = state.requests();
        let ids: Vec<&str> = requests.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2", "3"]);
    }
}
