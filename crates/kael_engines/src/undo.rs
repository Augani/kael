//! Document-scale transactional undo/redo history.
//!
//! A snapshot-based history over any cloneable document state (e.g. a project
//! document): each committed transaction can be
//! undone and redone, history is bounded, and rapid edits can be coalesced into
//! the current transaction so a drag doesn't flood the stack.

/// A bounded undo/redo history over a cloneable document state `T`.
pub struct UndoHistory<T> {
    past: Vec<T>,
    present: T,
    future: Vec<T>,
    limit: usize,
}

impl<T: Clone> UndoHistory<T> {
    /// Create a history seeded with `initial` state and a default depth limit of 100.
    pub fn new(initial: T) -> Self {
        Self {
            past: Vec::new(),
            present: initial,
            future: Vec::new(),
            limit: 100,
        }
    }

    /// Set the maximum number of undo steps retained (oldest are dropped).
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
        while self.past.len() > self.limit {
            self.past.remove(0);
        }
    }

    /// The current document state.
    pub fn current(&self) -> &T {
        &self.present
    }

    /// Mutable access to the current state without recording a transaction. Use
    /// for in-progress edits, then [`commit`](Self::commit) the result.
    pub fn current_mut(&mut self) -> &mut T {
        &mut self.present
    }

    /// Whether an undo is available.
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    /// Whether a redo is available.
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Number of undo steps available.
    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }

    /// Commit `next` as a new transaction: the current state becomes undoable and
    /// the redo stack is cleared.
    pub fn commit(&mut self, next: T) {
        let previous = std::mem::replace(&mut self.present, next);
        self.past.push(previous);
        if self.past.len() > self.limit {
            self.past.remove(0);
        }
        self.future.clear();
    }

    /// Apply a mutation as a single transaction (clone-mutate-commit).
    pub fn edit(&mut self, mutate: impl FnOnce(&mut T)) {
        let mut next = self.present.clone();
        mutate(&mut next);
        self.commit(next);
    }

    /// Replace the current state in place without pushing a new undo step — for
    /// coalescing rapid edits (a drag) into the current transaction.
    pub fn coalesce(&mut self, mutate: impl FnOnce(&mut T)) {
        mutate(&mut self.present);
        self.future.clear();
    }

    /// Undo the last transaction. Returns `false` if there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        if let Some(previous) = self.past.pop() {
            let current = std::mem::replace(&mut self.present, previous);
            self.future.push(current);
            true
        } else {
            false
        }
    }

    /// Redo the last undone transaction. Returns `false` if there is nothing to redo.
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.future.pop() {
            let current = std::mem::replace(&mut self.present, next);
            self.past.push(current);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_undo_redo_restores_states() {
        let mut history = UndoHistory::new(0i32);
        history.commit(1);
        history.commit(2);
        assert_eq!(*history.current(), 2);

        assert!(history.undo());
        assert_eq!(*history.current(), 1);
        assert!(history.undo());
        assert_eq!(*history.current(), 0);
        assert!(!history.undo());

        assert!(history.redo());
        assert_eq!(*history.current(), 1);
        assert!(history.redo());
        assert_eq!(*history.current(), 2);
        assert!(!history.redo());
    }

    #[test]
    fn edit_applies_one_transaction() {
        let mut history = UndoHistory::new(vec![1, 2, 3]);
        history.edit(|v| v.push(4));
        assert_eq!(history.current(), &vec![1, 2, 3, 4]);
        history.undo();
        assert_eq!(history.current(), &vec![1, 2, 3]);
    }

    #[test]
    fn commit_after_undo_clears_redo() {
        let mut history = UndoHistory::new(0);
        history.commit(1);
        history.commit(2);
        history.undo();
        assert!(history.can_redo());
        history.commit(9);
        assert!(!history.can_redo());
        assert_eq!(*history.current(), 9);
    }

    #[test]
    fn coalesce_does_not_grow_history() {
        let mut history = UndoHistory::new(0);
        history.commit(1);
        history.coalesce(|v| *v = 5);
        history.coalesce(|v| *v = 7);
        assert_eq!(*history.current(), 7);
        assert_eq!(history.undo_depth(), 1);
        history.undo();
        assert_eq!(*history.current(), 0);
    }

    #[test]
    fn limit_drops_oldest_steps() {
        let mut history = UndoHistory::new(0);
        history.set_limit(2);
        for value in 1..=5 {
            history.commit(value);
        }
        assert_eq!(history.undo_depth(), 2);
        history.undo();
        history.undo();
        assert!(!history.can_undo());
        // Only the two most-recent prior states were retained.
        assert_eq!(*history.current(), 3);
    }
}
