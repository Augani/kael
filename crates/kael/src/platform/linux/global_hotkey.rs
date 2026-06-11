#![allow(dead_code)]

use anyhow::Result;
use collections::HashMap;

use crate::Keystroke;

pub struct LinuxGlobalHotkey {
    registered: HashMap<u32, Keystroke>,
}

impl LinuxGlobalHotkey {
    pub fn new() -> Self {
        Self {
            registered: HashMap::default(),
        }
    }

    pub fn register(&mut self, id: u32, keystroke: &Keystroke) -> Result<()> {
        self.registered.insert(id, keystroke.clone());
        Ok(())
    }

    pub fn unregister(&mut self, id: u32) {
        self.registered.remove(&id);
    }
}

pub use crate::platform::global_hotkey_portal as portal;

#[cfg(feature = "x11")]
pub mod x11 {
    use super::*;
    use collections::HashSet;
    use std::rc::Rc;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{self, ConnectionExt as _, GrabMode, ModMask};
    use x11rb::xcb_ffi::XCBConnection;

    fn keystroke_to_x11_modmask(keystroke: &Keystroke) -> u16 {
        let mut mask = 0u16;
        if keystroke.modifiers.control {
            mask |= u16::from(ModMask::CONTROL);
        }
        if keystroke.modifiers.alt {
            mask |= u16::from(ModMask::M1);
        }
        if keystroke.modifiers.shift {
            mask |= u16::from(ModMask::SHIFT);
        }
        if keystroke.modifiers.platform {
            mask |= u16::from(ModMask::M4);
        }
        mask
    }

    fn keystroke_to_x11_keycode(keystroke: &Keystroke, xcb: &XCBConnection) -> Option<u8> {
        let setup = xcb.setup();
        let min_keycode = setup.min_keycode;
        let max_keycode = setup.max_keycode;

        let keysym = key_name_to_keysym(&keystroke.key)?;

        let reply = xcb
            .get_keyboard_mapping(min_keycode, max_keycode - min_keycode + 1)
            .ok()?
            .reply()
            .ok()?;

        let keysyms_per_keycode = reply.keysyms_per_keycode as usize;
        if keysyms_per_keycode == 0 {
            return None;
        }

        for i in 0..((max_keycode - min_keycode + 1) as usize) {
            let base = i * keysyms_per_keycode;
            for j in 0..keysyms_per_keycode {
                if reply.keysyms[base + j] == keysym {
                    return Some(min_keycode + i as u8);
                }
            }
        }

        None
    }

    fn key_name_to_keysym(key: &str) -> Option<u32> {
        match key.to_lowercase().as_str() {
            "a" => Some(0x61),
            "b" => Some(0x62),
            "c" => Some(0x63),
            "d" => Some(0x64),
            "e" => Some(0x65),
            "f" => Some(0x66),
            "g" => Some(0x67),
            "h" => Some(0x68),
            "i" => Some(0x69),
            "j" => Some(0x6a),
            "k" => Some(0x6b),
            "l" => Some(0x6c),
            "m" => Some(0x6d),
            "n" => Some(0x6e),
            "o" => Some(0x6f),
            "p" => Some(0x70),
            "q" => Some(0x71),
            "r" => Some(0x72),
            "s" => Some(0x73),
            "t" => Some(0x74),
            "u" => Some(0x75),
            "v" => Some(0x76),
            "w" => Some(0x77),
            "x" => Some(0x78),
            "y" => Some(0x79),
            "z" => Some(0x7a),
            "0" => Some(0x30),
            "1" => Some(0x31),
            "2" => Some(0x32),
            "3" => Some(0x33),
            "4" => Some(0x34),
            "5" => Some(0x35),
            "6" => Some(0x36),
            "7" => Some(0x37),
            "8" => Some(0x38),
            "9" => Some(0x39),
            "space" => Some(0x20),
            "enter" | "return" => Some(0xff0d),
            "tab" => Some(0xff09),
            "escape" => Some(0xff1b),
            "backspace" => Some(0xff08),
            "delete" => Some(0xffff),
            "insert" => Some(0xff63),
            "home" => Some(0xff50),
            "end" => Some(0xff57),
            "pageup" => Some(0xff55),
            "pagedown" => Some(0xff56),
            "left" => Some(0xff51),
            "up" => Some(0xff52),
            "right" => Some(0xff53),
            "down" => Some(0xff54),
            "f1" => Some(0xffbe),
            "f2" => Some(0xffbf),
            "f3" => Some(0xffc0),
            "f4" => Some(0xffc1),
            "f5" => Some(0xffc2),
            "f6" => Some(0xffc3),
            "f7" => Some(0xffc4),
            "f8" => Some(0xffc5),
            "f9" => Some(0xffc6),
            "f10" => Some(0xffc7),
            "f11" => Some(0xffc8),
            "f12" => Some(0xffc9),
            "-" => Some(0x2d),
            "=" => Some(0x3d),
            "[" => Some(0x5b),
            "]" => Some(0x5d),
            "\\" => Some(0x5c),
            ";" => Some(0x3b),
            "'" => Some(0x27),
            "`" => Some(0x60),
            "," => Some(0x2c),
            "." => Some(0x2e),
            "/" => Some(0x2f),
            _ => None,
        }
    }

    pub struct X11GlobalHotkey {
        inner: LinuxGlobalHotkey,
        keycodes: HashMap<u32, (u8, u16)>,
        pressed: HashSet<u32>,
    }

    impl X11GlobalHotkey {
        pub fn new() -> Self {
            Self {
                inner: LinuxGlobalHotkey::new(),
                keycodes: HashMap::default(),
                pressed: HashSet::default(),
            }
        }

        pub fn register(
            &mut self,
            id: u32,
            keystroke: &Keystroke,
            xcb: &Rc<XCBConnection>,
            root_window: xproto::Window,
        ) -> Result<()> {
            let keycode = keystroke_to_x11_keycode(keystroke, xcb).ok_or_else(|| {
                anyhow::anyhow!("Could not resolve keycode for key: {}", keystroke.key)
            })?;
            let modmask = keystroke_to_x11_modmask(keystroke);

            xcb.grab_key(
                false,
                root_window,
                modmask.into(),
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?
            .check()?;

            self.keycodes.insert(id, (keycode, modmask));
            self.inner.register(id, keystroke)
        }

        pub fn unregister(
            &mut self,
            id: u32,
            xcb: &Rc<XCBConnection>,
            root_window: xproto::Window,
        ) {
            if let Some((keycode, modmask)) = self.keycodes.remove(&id) {
                let _ = xcb.ungrab_key(keycode, root_window, modmask.into());
            }
            self.pressed.remove(&id);
            self.inner.unregister(id);
        }

        /// Find the hotkey ID matching a given keycode and modifier state.
        /// The modifier state from the event may include extra bits (e.g. NumLock),
        /// so we mask to only the bits we care about.
        pub fn find_hotkey_for_key(&self, keycode: u8, state: u16) -> Option<u32> {
            let relevant_mask = u16::from(ModMask::CONTROL)
                | u16::from(ModMask::M1)
                | u16::from(ModMask::SHIFT)
                | u16::from(ModMask::M4);
            let masked_state = state & relevant_mask;

            for (&id, &(kc, modmask)) in &self.keycodes {
                if kc == keycode && modmask == masked_state {
                    return Some(id);
                }
            }
            None
        }

        /// Mark a hotkey as pressed. Returns true if it was newly pressed.
        pub fn mark_pressed(&mut self, id: u32) -> bool {
            self.pressed.insert(id)
        }

        /// Mark a hotkey as released. Returns true if it was previously pressed.
        pub fn mark_released(&mut self, id: u32) -> bool {
            self.pressed.remove(&id)
        }

        /// Find which pressed hotkey IDs match a given keycode (for key-up events,
        /// where modifiers may already be released).
        pub fn find_pressed_for_keycode(&self, keycode: u8) -> Vec<u32> {
            self.pressed
                .iter()
                .filter(|&&id| {
                    self.keycodes
                        .get(&id)
                        .map_or(false, |&(kc, _)| kc == keycode)
                })
                .copied()
                .collect()
        }
    }
}

#[cfg(feature = "wayland")]
pub mod wayland {
    use super::portal::{
        BoundShortcut, PortalError, PortalShortcutState, SessionState, ShortcutRequest,
    };
    use super::*;
    use crate::BackgroundExecutor;
    use ashpd::desktop::Session;
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use ashpd::desktop::session::CreateSessionOptions;
    use calloop::channel::Sender;
    use smol::stream::StreamExt;

    #[derive(Debug)]
    pub enum HotkeyEvent {
        Activated(u32),
        Deactivated(u32),
        BindResult(Vec<BoundShortcut>),
        Failed(String),
    }

    enum Command {
        Register(u32, Keystroke, String),
        Unregister(u32),
    }

    pub struct WaylandGlobalHotkey {
        state: PortalShortcutState,
        command_tx: Option<flume::Sender<Command>>,
        worker_started: bool,
    }

    impl WaylandGlobalHotkey {
        pub fn new() -> Self {
            Self {
                state: PortalShortcutState::new(),
                command_tx: None,
                worker_started: false,
            }
        }

        pub fn session_state(&self) -> SessionState {
            self.state.state()
        }

        pub fn trigger_description(&self, id: u32) -> Option<String> {
            self.state.trigger_description(id).map(ToOwned::to_owned)
        }

        pub fn register(
            &mut self,
            id: u32,
            keystroke: &Keystroke,
            executor: &BackgroundExecutor,
            event_tx: Sender<HotkeyEvent>,
        ) -> Result<()> {
            if super::portal::keystroke_to_trigger(keystroke).is_none() {
                return Err(anyhow::anyhow!(
                    "Could not map key '{}' to an XDG shortcut trigger",
                    keystroke.key
                ));
            }

            let description = format!("Global shortcut {id}");
            self.state.register(id, keystroke, description.clone());

            if !self.worker_started {
                let (command_tx, command_rx) = flume::unbounded::<Command>();
                self.command_tx = Some(command_tx);
                self.worker_started = true;
                spawn_portal_worker(executor, command_rx, event_tx);
            }

            if let Some(tx) = &self.command_tx {
                tx.send(Command::Register(id, keystroke.clone(), description))
                    .map_err(|_| anyhow::anyhow!("GlobalShortcuts portal worker is gone"))?;
            }

            Ok(())
        }

        pub fn unregister(&mut self, id: u32) {
            self.state.unregister(id);
            if let Some(tx) = &self.command_tx {
                let _ = tx.send(Command::Unregister(id));
            }
        }

        pub fn on_activated(&self, shortcut_id: &str) -> Option<u32> {
            self.state.on_activated(shortcut_id)
        }

        pub fn on_deactivated(&self, shortcut_id: &str) -> Option<u32> {
            self.state.on_deactivated(shortcut_id)
        }

        pub fn apply_bind_result(&mut self, bound: &[BoundShortcut]) {
            self.state.apply_bind_result(bound);
        }
    }

    impl Default for WaylandGlobalHotkey {
        fn default() -> Self {
            Self::new()
        }
    }

    async fn bind_via_portal(
        portal: &GlobalShortcuts,
        session: &Session<GlobalShortcuts>,
        requests: &[ShortcutRequest],
    ) -> std::result::Result<Vec<BoundShortcut>, PortalError> {
        let shortcuts: Vec<NewShortcut> = requests
            .iter()
            .map(|req| {
                let shortcut = NewShortcut::new(req.id.clone(), req.description.clone());
                match req.preferred_trigger.as_deref() {
                    Some(trigger) => shortcut.preferred_trigger(Some(trigger)),
                    None => shortcut,
                }
            })
            .collect();

        let request = portal
            .bind_shortcuts(session, &shortcuts, None, Default::default())
            .await
            .map_err(|err| PortalError::Bind(err.to_string()))?;
        let response = request
            .response()
            .map_err(|err| PortalError::Bind(err.to_string()))?;
        Ok(response
            .shortcuts()
            .iter()
            .map(|shortcut| BoundShortcut {
                id: shortcut.id().to_string(),
                trigger_description: shortcut.trigger_description().to_string(),
            })
            .collect())
    }

    fn spawn_portal_worker(
        executor: &BackgroundExecutor,
        command_rx: flume::Receiver<Command>,
        event_tx: Sender<HotkeyEvent>,
    ) {
        let executor = executor.clone();
        executor
            .clone()
            .spawn(async move {
                let mut state = PortalShortcutState::new();

                let portal = match GlobalShortcuts::new().await {
                    Ok(portal) => portal,
                    Err(err) => {
                        let _ = event_tx.send(HotkeyEvent::Failed(
                            PortalError::Unavailable(err.to_string()).to_string(),
                        ));
                        return;
                    }
                };

                let session = match portal.create_session(CreateSessionOptions::default()).await {
                    Ok(session) => session,
                    Err(err) => {
                        let _ = event_tx.send(HotkeyEvent::Failed(
                            PortalError::Unavailable(err.to_string()).to_string(),
                        ));
                        return;
                    }
                };

                let activated_tx = event_tx.clone();
                if let Ok(mut activated) = portal.receive_activated().await {
                    executor
                        .spawn(async move {
                            while let Some(signal) = activated.next().await {
                                if let Some(id) =
                                    super::portal::parse_shortcut_id(signal.shortcut_id())
                                {
                                    let _ = activated_tx.send(HotkeyEvent::Activated(id));
                                }
                            }
                        })
                        .detach();
                }

                let deactivated_tx = event_tx.clone();
                if let Ok(mut deactivated) = portal.receive_deactivated().await {
                    executor
                        .spawn(async move {
                            while let Some(signal) = deactivated.next().await {
                                if let Some(id) =
                                    super::portal::parse_shortcut_id(signal.shortcut_id())
                                {
                                    let _ = deactivated_tx.send(HotkeyEvent::Deactivated(id));
                                }
                            }
                        })
                        .detach();
                }

                while let Ok(command) = command_rx.recv_async().await {
                    match command {
                        Command::Register(id, keystroke, description) => {
                            state.register(id, &keystroke, description);
                        }
                        Command::Unregister(id) => {
                            state.unregister(id);
                        }
                    }

                    if state.is_empty() {
                        continue;
                    }

                    let requests = state.requests();
                    match bind_via_portal(&portal, &session, &requests).await {
                        Ok(bound) => {
                            state.apply_bind_result(&bound);
                            let _ = event_tx.send(HotkeyEvent::BindResult(bound));
                        }
                        Err(err) => {
                            let _ = event_tx.send(HotkeyEvent::Failed(err.to_string()));
                        }
                    }
                }

                let _ = session.close().await;
            })
            .detach();
    }
}
