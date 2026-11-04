//! Undo/redo history. Edits that follow each other quickly (or one long drag)
//! are grouped into a single step.

use std::time::{Duration, Instant};

/// Edits closer together than this form one undo step.
const GROUP: Duration = Duration::from_millis(600);
const DEPTH: usize = 50;

pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    /// The current step stays open for more edits until this instant.
    open_until: Option<Instant>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), open_until: None }
    }
}

impl<T> History<T> {
    /// An edit was made; `before` is the state it started from. Starts a new
    /// step unless the current one is still open.
    pub fn record(&mut self, before: T, now: Instant) {
        if !self.is_open(now) {
            self.undo.push(before);
            if self.undo.len() > DEPTH {
                self.undo.remove(0);
            }
        }
        self.open_until = Some(now + GROUP);
        self.redo.clear();
    }

    /// The next edit starts a new step.
    pub fn close_step(&mut self) {
        self.open_until = None;
    }

    /// Keep an open step open (while a drag goes on, however long it takes).
    pub fn extend_step(&mut self, now: Instant) {
        if self.open_until.is_some() {
            self.open_until = Some(now + GROUP);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Go back one step. `restore` puts a state back and says whether that
    /// worked; `current` is what the redo step returns to. Returns whether a
    /// state was restored.
    pub fn undo(&mut self, current: T, restore: impl FnOnce(&T) -> bool) -> bool {
        let done = Self::step(&mut self.undo, &mut self.redo, current, restore);
        if done {
            self.open_until = None;
        }
        done
    }

    /// Go forward one step; see `undo`.
    pub fn redo(&mut self, current: T, restore: impl FnOnce(&T) -> bool) -> bool {
        let done = Self::step(&mut self.redo, &mut self.undo, current, restore);
        if done {
            self.open_until = None;
        }
        done
    }

    fn step(from: &mut Vec<T>, to: &mut Vec<T>, current: T, restore: impl FnOnce(&T) -> bool) -> bool {
        let Some(target) = from.pop() else { return false };
        let done = restore(&target);
        if done {
            to.push(current);
        }
        done
    }

    fn is_open(&self, now: Instant) -> bool {
        self.open_until.is_some_and(|t| now < t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_edits_form_one_step() {
        let mut h = History::default();
        let t = Instant::now();
        h.record(0, t);
        h.record(1, t + Duration::from_millis(100));
        h.record(2, t + Duration::from_secs(5));
        assert_eq!(h.undo, [0, 2]);
    }

    #[test]
    fn undo_and_redo_swap_states() {
        let mut h = History::default();
        let mut state = 1;
        h.record(0, Instant::now());
        h.undo(state, |&s| {
            state = s;
            true
        });
        assert_eq!((state, h.can_undo()), (0, false));
        h.redo(state, |&s| {
            state = s;
            true
        });
        assert_eq!((state, h.can_undo()), (1, true));
    }

    #[test]
    fn failed_restore_keeps_redo_empty() {
        let mut h = History::default();
        h.record(0, Instant::now());
        assert!(!h.undo(1, |_| false));
        assert!(h.redo.is_empty());
    }
}
