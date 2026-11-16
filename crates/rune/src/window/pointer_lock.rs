//! The window's side of `El::pointer_lock`: grabbing the OS cursor when the runtime's lock
//! captures and letting go when it ends, and the mouse's raw motion while it holds.
//!
//! `CursorGrabMode::Locked` (macOS, Wayland) keeps the cursor where it is. Where only
//! `Confined` works (X11, Windows) the hidden cursor wanders inside the window, so it's put
//! back where the press took it when the lock ends.

use winit::dpi::LogicalPosition;
use winit::window::CursorGrabMode;

use crate::runtime::{LockEffect, Runtime};
use super::Win;

impl Win {
    /// Carries out the grab or release the runtime owes the OS cursor, if any.
    pub(super) fn apply_pointer_lock<S: 'static>(&mut self, rt: &mut Runtime<S>) {
        let Some(effect) = rt.take_pointer_lock_effect() else { return };
        match effect {
            LockEffect::Grab => {
                let at = rt.lock_point().map(|(x, y)| (x * rt.zoom(), y * rt.zoom()));
                self.grab = [CursorGrabMode::Locked, CursorGrabMode::Confined].into_iter()
                    .find(|&mode| self.window.set_cursor_grab(mode).is_ok())
                    .map(|mode| (mode, at));
                // Motion still comes from the device; only the cursor isn't held.
                if self.grab.is_none() { eprintln!("rune: pointer lock: the window couldn't grab the cursor"); }
            }
            LockEffect::Ungrab => {
                let _ = self.window.set_cursor_grab(CursorGrabMode::None);
                if let Some((CursorGrabMode::Confined, Some((x, y)))) = self.grab.take() {
                    let _ = self.window.set_cursor_position(LogicalPosition::new(x as f64, y as f64));
                }
            }
        }
    }
}

/// Raw device motion in view points: macOS and Wayland report points already, X11 and Windows
/// device counts, which come near pixels.
pub(super) fn motion_points(delta: (f64, f64), wayland: bool, scale: f64, zoom: f32) -> (f32, f32) {
    let per_point = if cfg!(target_os = "macos") || wayland { 1.0 } else { scale };
    let k = per_point as f32 * zoom;
    ((delta.0 as f32) / k, (delta.1 as f32) / k)
}

#[cfg(test)]
mod tests {
    use super::motion_points;

    #[test]
    fn device_counts_become_view_points() {
        let x11 = motion_points((4.0, -2.0), false, 2.0, 1.0);
        if cfg!(target_os = "macos") { assert_eq!(x11, (4.0, -2.0)) } else { assert_eq!(x11, (2.0, -1.0)) }
        assert_eq!(motion_points((4.0, -2.0), true, 2.0, 2.0), (2.0, -1.0), "points over the view's zoom");
    }
}
