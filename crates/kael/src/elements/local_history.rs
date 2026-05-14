use crate::{
    Action, App, DispatchPhase, FocusHandle, Global, KeyBinding, KeyContext, Redo, Undo,
    UndoRedoManager, UndoableChange, Window,
};
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    fmt::{self, Debug},
    marker::PhantomData,
    rc::Rc,
};

pub(crate) const LOCAL_UNDO_REDO_CONTEXT: &str = "LocalUndoRedo";

#[derive(Default)]
struct LocalUndoRedoBindingsInstalled;

impl Global for LocalUndoRedoBindingsInstalled {}

pub(crate) fn ensure_local_undo_redo_bindings(cx: &mut App) {
    if cx.has_global::<LocalUndoRedoBindingsInstalled>() {
        return;
    }

    cx.bind_keys([
        KeyBinding::new("secondary-z", Undo, Some(LOCAL_UNDO_REDO_CONTEXT)),
        KeyBinding::new("secondary-shift-z", Redo, Some(LOCAL_UNDO_REDO_CONTEXT)),
        KeyBinding::new("secondary-y", Redo, Some(LOCAL_UNDO_REDO_CONTEXT)),
    ]);
    cx.set_global(LocalUndoRedoBindingsInstalled);
}

pub(crate) fn local_undo_redo_key_context() -> KeyContext {
    KeyContext::parse(LOCAL_UNDO_REDO_CONTEXT).expect("valid local undo/redo context")
}

pub(crate) fn register_focused_action_handler<A: Action + 'static>(
    window: &mut Window,
    focus_handle: FocusHandle,
    handler: impl Fn(&A, &mut Window, &mut App) + 'static,
) {
    window.on_action(TypeId::of::<A>(), move |action, phase, window, cx| {
        if phase != DispatchPhase::Bubble || !focus_handle.is_focused(window) {
            return;
        }

        let Some(action) = action.downcast_ref::<A>() else {
            return;
        };

        handler(action, window, cx);
        cx.stop_propagation();
    });
}

pub(crate) fn register_focused_action_handler_when<A: Action + 'static>(
    window: &mut Window,
    enabled: bool,
    focus_handle: FocusHandle,
    handler: impl Fn(&A, &mut Window, &mut App) + 'static,
) {
    if enabled {
        register_focused_action_handler(window, focus_handle, handler);
    }
}

#[cfg(test)]
pub(crate) struct ValueHistory<T> {
    manager: UndoRedoManager,
    description: String,
    _marker: PhantomData<T>,
}

#[cfg(test)]
impl<T: 'static> Default for ValueHistory<T> {
    fn default() -> Self {
        Self::new("Edit")
    }
}

#[cfg(test)]
impl<T> Debug for ValueHistory<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValueHistory")
            .field("description", &self.description)
            .field("can_undo", &self.manager.can_undo())
            .field("can_redo", &self.manager.can_redo())
            .finish()
    }
}

#[cfg(test)]
struct ValueChange<T> {
    previous: T,
    next: T,
    description: String,
}

#[cfg(test)]
impl<T> ValueChange<T> {
    fn new(previous: T, next: T, description: String) -> Self {
        Self {
            previous,
            next,
            description,
        }
    }
}

#[cfg(test)]
impl<T> Debug for ValueChange<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValueChange")
            .field("description", &self.description)
            .finish()
    }
}

#[cfg(test)]
impl<T: 'static> UndoableChange for ValueChange<T> {
    fn apply(&mut self) {}

    fn revert(&mut self) {}

    fn description(&self) -> &str {
        &self.description
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct WindowValueChange<T> {
    previous: T,
    next: T,
    description: String,
    source_id: crate::FocusId,
}

impl<T> WindowValueChange<T> {
    fn new(previous: T, next: T, description: String, source_id: crate::FocusId) -> Self {
        Self {
            previous,
            next,
            description,
            source_id,
        }
    }
}

impl<T> Debug for WindowValueChange<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowValueChange")
            .field("description", &self.description)
            .field("source_id", &self.source_id)
            .finish()
    }
}

impl<T: 'static> UndoableChange for WindowValueChange<T> {
    fn apply(&mut self) {}

    fn revert(&mut self) {}

    fn description(&self) -> &str {
        &self.description
    }

    fn source_id(&self) -> Option<crate::FocusId> {
        Some(self.source_id)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl<T: 'static> ValueHistory<T> {
    pub(crate) fn new(description: impl Into<String>) -> Self {
        Self {
            manager: UndoRedoManager::default(),
            description: description.into(),
            _marker: PhantomData,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.manager.clear();
    }

    pub(crate) fn can_undo(&self) -> bool {
        self.manager.can_undo()
    }

    pub(crate) fn can_redo(&self) -> bool {
        self.manager.can_redo()
    }

    pub(crate) fn record(&mut self, previous: T, next: T) {
        self.manager.push(Box::new(ValueChange::new(
            previous,
            next,
            self.description.clone(),
        )));
    }

    pub(crate) fn replace_last(&mut self, previous: T, next: T) -> bool {
        self.manager.replace_last(Box::new(ValueChange::new(
            previous,
            next,
            self.description.clone(),
        )))
    }

    pub(crate) fn undo(&mut self) -> Option<T>
    where
        T: Clone,
    {
        let change = self.manager.undo()?;
        let change = change
            .as_any()
            .downcast_ref::<ValueChange<T>>()
            .expect("value history changes stay typed");
        Some(change.previous.clone())
    }

    pub(crate) fn redo(&mut self) -> Option<T>
    where
        T: Clone,
    {
        let change = self.manager.redo()?;
        let change = change
            .as_any()
            .downcast_ref::<ValueChange<T>>()
            .expect("value history changes stay typed");
        Some(change.next.clone())
    }
}

pub(crate) struct WindowValueHistory<T> {
    manager: Rc<RefCell<UndoRedoManager>>,
    description: String,
    source_id: crate::FocusId,
    _marker: PhantomData<T>,
}

impl<T> Debug for WindowValueHistory<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let manager = self.manager.borrow();
        f.debug_struct("WindowValueHistory")
            .field("description", &self.description)
            .field("source_id", &self.source_id)
            .field("can_undo", &manager.can_undo_for_source(self.source_id))
            .field("can_redo", &manager.can_redo_for_source(self.source_id))
            .finish()
    }
}

impl<T: 'static> WindowValueHistory<T> {
    pub(crate) fn new(
        manager: Rc<RefCell<UndoRedoManager>>,
        focus_handle: &FocusHandle,
        description: impl Into<String>,
    ) -> Self {
        Self {
            manager,
            description: description.into(),
            source_id: focus_handle.id,
            _marker: PhantomData,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.manager.borrow_mut().clear_for_source(self.source_id);
    }

    pub(crate) fn can_undo(&self) -> bool {
        self.manager.borrow().has_undo_for_source(self.source_id)
    }

    pub(crate) fn can_redo(&self) -> bool {
        self.manager.borrow().has_redo_for_source(self.source_id)
    }

    pub(crate) fn record(&mut self, previous: T, next: T) {
        self.manager
            .borrow_mut()
            .push(Box::new(WindowValueChange::new(
                previous,
                next,
                self.description.clone(),
                self.source_id,
            )));
    }

    pub(crate) fn replace_last(&mut self, previous: T, next: T) -> bool {
        let manager = self.manager.borrow();
        if !manager.can_undo_for_source(self.source_id) {
            return false;
        }
        drop(manager);

        self.manager
            .borrow_mut()
            .replace_last(Box::new(WindowValueChange::new(
                previous,
                next,
                self.description.clone(),
                self.source_id,
            )))
    }

    pub(crate) fn undo(&mut self) -> Option<T>
    where
        T: Clone,
    {
        let manager = self.manager.borrow();
        if !manager.can_undo_for_source(self.source_id) {
            return None;
        }
        drop(manager);

        let mut manager = self.manager.borrow_mut();
        let change = manager.undo_for_source(self.source_id)?;
        let change = change
            .as_any()
            .downcast_ref::<WindowValueChange<T>>()
            .expect("window value history changes stay typed");
        Some(change.previous.clone())
    }

    pub(crate) fn redo(&mut self) -> Option<T>
    where
        T: Clone,
    {
        let manager = self.manager.borrow();
        if !manager.can_redo_for_source(self.source_id) {
            return None;
        }
        drop(manager);

        let mut manager = self.manager.borrow_mut();
        let change = manager.redo_for_source(self.source_id)?;
        let change = change
            .as_any()
            .downcast_ref::<WindowValueChange<T>>()
            .expect("window value history changes stay typed");
        Some(change.next.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::ValueHistory;

    #[test]
    fn value_history_undo_redo_round_trips_values() {
        let mut history = ValueHistory::new("Number change");
        history.record(0, 1);
        history.record(1, 2);
        history.record(2, 3);

        assert_eq!(history.undo(), Some(2));
        assert_eq!(history.undo(), Some(1));
        assert_eq!(history.redo(), Some(2));
        assert_eq!(history.redo(), Some(3));
    }

    #[test]
    fn value_history_clears_redo_on_new_record() {
        let mut history = ValueHistory::new("Number change");
        history.record(0, 1);
        history.record(1, 2);

        assert_eq!(history.undo(), Some(1));
        history.record(1, 9);

        assert_eq!(history.redo(), None);
    }

    #[test]
    fn value_history_replace_last_overwrites_latest_entry() {
        let mut history = ValueHistory::new("Number change");
        history.record(0, 1);
        history.record(1, 2);

        assert!(history.replace_last(1, 3));
        assert_eq!(history.undo(), Some(1));
        assert_eq!(history.redo(), Some(3));
    }
}
