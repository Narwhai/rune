//! Pointer lock: a press on an element that asks for it ([`El::pointer_lock`]) holds the
//! pointer still and hidden, and the device's raw motion goes to the element as deltas, past
//! where the window or the screen would stop an absolute pointer. A game or 3D view looks around
//! this way until Escape ([`PointerLockRelease::Escape`]); a knob or a number scrubber turns
//! for as long as the button is held ([`PointerLockRelease::Release`]).
//!
//! The runtime owns the lock as a machine: a press on an element that asks takes it, and
//! Escape, focus loss, the element going away or (for some) the release end it. The window grabs and lets go of the OS cursor through the
//! effect owed to it ([`Runtime::take_pointer_lock_effect`]). Desktop windows only: on the web
//! and iOS nothing grabs, so no element ever captures.

use crate::machine::Effects;

use super::*;

/// When a captured lock lets go besides Escape, focus loss and the element going away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PointerLockRelease {
    /// Only then: a press captures and the pointer stays held after the button comes up,
    /// as a game's "click to look around".
    #[default]
    Escape,
    /// Also when the button that captured comes up: a knob or a scrubber held while dragged.
    Release,
}

/// Where the lock stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) enum LockState {
    /// The pointer is free.
    #[default]
    Off,
    /// The pointer is held for `path`, the press that took it at `at` (window points).
    Locked { path: String, release: PointerLockRelease, at: (f32, f32) },
}

#[derive(Debug)]
pub(super) enum LockEvent<'a> {
    /// A primary press at `at` on `path`, which asks for the lock.
    Pressed { path: &'a str, release: PointerLockRelease, at: (f32, f32) },
    /// After a frame: whether the element holding the lock still asks for it, and how.
    Asked(Option<PointerLockRelease>),
    /// The primary button came up.
    ButtonUp,
    /// Escape, focus loss or a pointer cancel.
    Released,
}

/// What the window does with the OS cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LockEffect {
    Grab,
    Ungrab,
}

pub(super) fn step(state: &mut LockState, event: LockEvent) -> Effects<LockEffect> {
    match (&*state, event) {
        (LockState::Off, LockEvent::Pressed { path, release, at }) => {
            *state = LockState::Locked { path: path.to_owned(), release, at };
            Effects::one(LockEffect::Grab)
        }
        // It changed how it lets go: the new way holds from now.
        (LockState::Locked { path, at, .. }, LockEvent::Asked(Some(release))) => {
            *state = LockState::Locked { path: path.clone(), release, at: *at };
            Effects::none()
        }
        (LockState::Locked { .. }, LockEvent::Asked(None) | LockEvent::Released)
        | (LockState::Locked { release: PointerLockRelease::Release, .. }, LockEvent::ButtonUp) => {
            *state = LockState::Off;
            Effects::one(LockEffect::Ungrab)
        }
        _ => Effects::none(),
    }
}

/// The lock and the cursor change owed to the window.
#[derive(Debug, Default)]
pub(super) struct PointerLock {
    state: LockState,
    /// Grab and Ungrab alternate, so a pending opposite cancels out: at most one change is owed,
    /// and a grab the window never made needs no release.
    owed: Option<LockEffect>,
}

impl PointerLock {
    fn locked(&self) -> Option<(&str, (f32, f32))> {
        match &self.state {
            LockState::Locked { path, at, .. } => Some((path, *at)),
            LockState::Off => None,
        }
    }

    /// Steps the machine; true when whether it's locked changed.
    fn step(&mut self, event: LockEvent) -> bool {
        let was = self.locked().is_some();
        for effect in step(&mut self.state, event) {
            self.owed = match self.owed {
                Some(owed) if owed != effect => None,
                _ => Some(effect),
            };
        }
        was != self.locked().is_some()
    }
}

impl<S: 'static> Runtime<S> {
    /// The pointer is held by an element's [`El::pointer_lock`].
    pub fn pointer_locked(&self) -> bool { self.pointer_lock.locked().is_some() }

    /// Where a captured lock pins the pointer: every press, release and move lands here.
    pub(crate) fn lock_point(&self) -> Option<(f32, f32)> { self.pointer_lock.locked().map(|(_, at)| at) }

    fn step_pointer_lock(&mut self, event: LockEvent) -> bool {
        let flipped = self.pointer_lock.step(event);
        if flipped { self.dirty = true; }
        flipped
    }

    /// How element `i` asks for the lock: drawn, reachable past any modal, not leaving.
    fn asks_pointer_lock(&self, i: usize) -> Option<PointerLockRelease> {
        let lock = self.nodes[i].handlers.get()?.pointer_lock.as_ref()?;
        let drawn = self.hits.iter().any(|&(id, _)| id == self.nodes[i].id);
        (drawn && self.allowed(i) && !self.subtree_exiting(i)).then_some(lock.release)
    }

    /// After a frame: the element holding the lock lets go once it no longer asks.
    pub(super) fn sync_pointer_lock(&mut self) {
        let Some((path, _)) = self.pointer_lock.locked() else { return };
        let asked = self.live_index(path).and_then(|i| self.asks_pointer_lock(i));
        self.step_pointer_lock(LockEvent::Asked(asked));
    }

    /// A primary press at (x, y) whose target is element `i`: it captures if `i` asks.
    pub(super) fn pointer_lock_press(&mut self, i: usize, x: f32, y: f32) {
        if !self.os_pointer_lock { return; }
        let Some(release) = self.asks_pointer_lock(i) else { return };
        let path = self.nodes[i].path.clone();
        self.step_pointer_lock(LockEvent::Pressed { path: &path, release, at: (x, y) });
    }

    /// The primary button came up: a [`PointerLockRelease::Release`] lock lets go.
    pub(super) fn pointer_lock_button_up(&mut self) { self.step_pointer_lock(LockEvent::ButtonUp); }

    /// Escape, focus loss or a cancel lets go of a captured lock; true when one was held.
    pub(crate) fn release_pointer_lock(&mut self) -> bool { self.step_pointer_lock(LockEvent::Released) }

    /// Escape: cancels a held drag and lets go of a held lock; true when either was held.
    pub(super) fn escape_pointer(&mut self) -> bool {
        let dragging = self.cancel_drag();
        self.release_pointer_lock() || dragging
    }

    /// What the window owes the OS cursor since it last asked.
    pub(crate) fn take_pointer_lock_effect(&mut self) -> Option<LockEffect> {
        self.sync_pointer_lock();
        self.pointer_lock.owed.take()
    }

    /// Whether this host can hold the pointer. Without, no element ever captures.
    pub(crate) fn set_os_pointer_lock(&mut self, supported: bool) { self.os_pointer_lock = supported; }

    /// Raw device motion of (dx, dy) points (right and down positive) while the pointer is
    /// locked; dropped otherwise. Motion faster than frames adds up into one delivery.
    pub fn pointer_motion(&mut self, dx: f32, dy: f32) {
        let Some((path, _)) = self.pointer_lock.locked() else { return };
        if dx == 0.0 && dy == 0.0 { return; }
        if let Some(Action::PointerLockMotion(p, d)) = self.queue.last_mut() && p == path {
            d[0] += dx;
            d[1] += dy;
            return;
        }
        let path = path.to_owned();
        self.queue.push(Action::PointerLockMotion(path, [dx, dy]));
    }
}

#[cfg(test)]
#[path = "pointer_lock_tests.rs"]
mod tests;
