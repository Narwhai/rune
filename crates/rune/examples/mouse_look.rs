//! Mouse look, as a game does it (`El::pointer_lock`): click the view and the cursor hides and
//! stays put while the mouse turns the camera, as far as you move it, round and round, until
//! Escape. Escape reaches the app only when nothing is locked: a second Escape quits.
//!
//! The room is wireframe: a floor grid, four walls with a doorway on each, and a pillar in each
//! corner, projected each frame from the camera's yaw and pitch.
//!
//! What to check by hand:
//! - The cursor vanishes on the click and comes back where it was after Escape.
//! - Turning keeps going past the screen's edge: spin several full turns in one direction
//!   (the turn counter climbs).
//! - Clicks and the wheel while locked land on the view (the click counter, the field of view),
//!   not the "Reset" button under the cursor.
//! - Switching away (Alt-Tab, another workspace) lets go; coming back doesn't lock again until
//!   the next click.
//!
//! cargo run -p rune --example mouse_look
use std::f32::consts::{PI, TAU};

use rune::{AppSpec, Color, Cx, El, Key, KeyEvent, LineCap, Shape, Tx, WheelEvent, canvas, col, el, row, text};

const W: f32 = 720.0;
const H: f32 = 440.0;
/// The room's edges and the locked hint.
const MINT: Color = Color { r: 0.55, g: 0.85, b: 0.75, a: 1.0 };
/// Radians per point of mouse motion.
const SENSITIVITY: f32 = 0.004;

struct Look {
    yaw: f32,
    pitch: f32,
    /// Horizontal field of view in degrees, which the wheel changes.
    fov: f32,
    /// Total yaw turned, in radians, to show turning never stops at an edge.
    turned: f32,
    /// Clicks on the view while it's locked.
    clicks: u32,
    resets: u32,
}

impl Default for Look {
    fn default() -> Self { Look { yaw: 0.0, pitch: 0.0, fov: 90.0, turned: 0.0, clicks: 0, resets: 0 } }
}

impl Look {
    fn turn(&mut self, [dx, dy]: [f32; 2]) {
        self.yaw = (self.yaw + dx * SENSITIVITY).rem_euclid(TAU);
        self.turned += dx * SENSITIVITY;
        self.pitch = (self.pitch - dy * SENSITIVITY).clamp(-1.4, 1.4);
    }

    /// World point `p` on screen, or None behind the camera.
    fn project(&self, p: [f32; 3]) -> Option<[f32; 2]> {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        // Into camera space: yaw about y, then pitch about x. The camera looks down +z.
        let x = p[0] * cy - p[2] * sy;
        let z = p[0] * sy + p[2] * cy;
        let y = p[1] * cp - z * sp;
        let z = p[1] * sp + z * cp;
        if z < 0.05 { return None; }
        let f = (W / 2.0) / (self.fov.to_radians() / 2.0).tan();
        Some([W / 2.0 + x / z * f, H / 2.0 - y / z * f])
    }
}

/// The room's edges, world units, the eye at the origin 1.6 above the floor.
fn room() -> Vec<([f32; 3], [f32; 3])> {
    let (r, floor, ceiling) = (6.0, -1.6, 2.4);
    let mut lines = Vec::new();
    // The floor grid.
    for i in -6..=6 {
        let v = i as f32;
        lines.push(([v, floor, -r], [v, floor, r]));
        lines.push(([-r, floor, v], [r, floor, v]));
    }
    // Four walls, each with a doorway: its top edge, and the floor-to-ceiling edges either side.
    for k in 0..4 {
        let a = k as f32 * PI / 2.0;
        let at = |u: f32, y: f32| {
            let (s, c) = a.sin_cos();
            [r * c - u * s, y, r * s + u * c]
        };
        lines.push((at(-r, ceiling), at(r, ceiling)));
        lines.push((at(-r, floor), at(-1.0, floor)));
        lines.push((at(1.0, floor), at(r, floor)));
        lines.push((at(-1.0, floor), at(-1.0, 0.6)));
        lines.push((at(1.0, floor), at(1.0, 0.6)));
        lines.push((at(-1.0, 0.6), at(1.0, 0.6)));
        lines.push((at(-r, floor), at(-r, ceiling)));
    }
    // A pillar in each corner, part way in.
    for (x, z) in [(3.5, 3.5), (-3.5, 3.5), (-3.5, -3.5), (3.5, -3.5)] {
        let s = 0.4;
        for (dx, dz) in [(-s, -s), (s, -s), (s, s), (-s, s)] {
            lines.push(([x + dx, floor, z + dz], [x + dx, ceiling, z + dz]));
        }
    }
    lines
}

/// Splits a segment where it crosses the near plane, so walls beside and behind the camera
/// don't vanish whole.
fn clip(s: &Look, a: [f32; 3], b: [f32; 3]) -> Option<([f32; 2], [f32; 2])> {
    let depth = |p: [f32; 3]| {
        let (sy, cy) = s.yaw.sin_cos();
        let (sp, cp) = s.pitch.sin_cos();
        let z = p[0] * sy + p[2] * cy;
        p[1] * sp + z * cp
    };
    let (da, db) = (depth(a), depth(b));
    let near = 0.05;
    if da < near && db < near { return None; }
    let lerp = |t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
    let (a, b) = match (da < near, db < near) {
        (true, false) => (lerp((near + 1e-3 - da) / (db - da)), b),
        (false, true) => (a, lerp((near + 1e-3 - da) / (db - da))),
        _ => (a, b),
    };
    Some((s.project(a)?, s.project(b)?))
}

fn scene(s: &Look) -> Shape {
    let edge = MINT;
    let mut shape = Shape::new();
    for (a, b) in room() {
        if let Some((a, b)) = clip(s, a, b) {
            shape = shape.line(a, b, 1.5, LineCap::Round, edge);
        }
    }
    // A crosshair at the centre.
    let c = [W / 2.0, H / 2.0];
    shape.line([c[0] - 8.0, c[1]], [c[0] + 8.0, c[1]], 1.5, LineCap::Butt, Color::WHITE)
        .line([c[0], c[1] - 8.0], [c[0], c[1] + 8.0], 1.5, LineCap::Butt, Color::WHITE)
}

fn view(s: &Look, cx: &mut Cx<Look>) -> El {
    let locked = cx.pointer_locked();
    let hint = if locked { "Move the mouse to look · wheel: field of view · Esc: let go" } else { "Click the view to look around · Esc quits" };
    let world = el().key("view").size(W, H).fill(Color { r: 0.06, g: 0.07, b: 0.09, a: 1.0 }).clip_soft(0.0)
        .on_press(|s: &mut Look| s.clicks += 1)
        .on_wheel(|s: &mut Look, e: WheelEvent, _| {
            s.fov = (s.fov + e.dy * 0.1).clamp(40.0, 120.0);
            true
        })
        .pointer_lock(|s: &mut Look, d, _| s.turn(d))
        .child(canvas(scene(s)).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)))
        // Under where a press would leave the cursor, to show a locked click doesn't reach it.
        .child(el().key("reset").size(90.0, 30.0).radius(6.0).pin(None, None, Some(12.0), Some(12.0)).center()
            .fill(Color::WHITE.alpha(0.12)).hover(Color::WHITE.alpha(0.1))
            .on_press(|s: &mut Look| { s.resets += 1; s.yaw = 0.0; s.pitch = 0.0; })
            .child(text("Reset").color(Color::WHITE)));
    let stat = |label: String| text(label).color(Color::WHITE.alpha(0.75));
    col().w_full().h_full().fill(Color { r: 0.1, g: 0.11, b: 0.13, a: 1.0 }).center().gap(10.0)
        .child(text(hint).color(if locked { MINT } else { Color::WHITE }))
        .child(world)
        .child(row().gap(18.0)
            .child(stat(format!("yaw {:.0}°", s.yaw.to_degrees())))
            .child(stat(format!("pitch {:.0}°", s.pitch.to_degrees())))
            .child(stat(format!("turned {:+.2} turns", s.turned / TAU)))
            .child(stat(format!("fov {:.0}°", s.fov)))
            .child(stat(format!("locked clicks {}", s.clicks)))
            .child(stat(format!("resets {}", s.resets))))
}

/// The app hears only the Escape a lock didn't take.
fn on_key(_: &mut Look, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.key != Key::Escape { return false; }
    tx.quit();
    true
}

fn main() {
    rune::run(AppSpec { on_key: Some(on_key), ..AppSpec::new("Mouse look", (W + 40.0, H + 90.0), Look::default, view) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Mods, Runtime};

    fn runtime() -> Runtime<Look> {
        let mut rt = Runtime::new(Look::default(), view, (W + 40.0, H + 90.0), Env::new(true, 1.0));
        rt.on_key = Some(|s, ev, _| { if ev.key == Key::Escape { s.resets += 100; } true });
        rt.window_focus(true);
        rt.frame(0.0);
        rt
    }

    #[test]
    fn a_click_locks_and_the_mouse_turns_the_camera_past_any_edge() {
        let mut rt = runtime();
        let (x, y) = rt.find("view").unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
        assert!(rt.pointer_locked());
        // Three full turns: far more travel than any screen.
        let per_turn = TAU / SENSITIVITY;
        for _ in 0..30 { rt.pointer_motion(per_turn / 10.0, 0.0); rt.frame(1.0 / 60.0); }
        assert!((rt.state.turned / TAU - 3.0).abs() < 1e-3, "{}", rt.state.turned / TAU);
        assert_eq!(rt.pointer(), Some((x, y)), "the cursor stayed put");
        // The release doesn't let go: only Escape does, and the app doesn't hear it.
        rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
        rt.frame(0.0);
        assert!(!rt.pointer_locked());
        assert_eq!(rt.state.resets, 0);
    }

    #[test]
    fn a_locked_click_over_the_reset_button_lands_on_the_view() {
        let mut rt = runtime();
        let (x, y) = rt.find("view").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
        let (rx, ry) = rt.find("reset").unwrap();
        rt.pointer_down(rx, ry);
        rt.pointer_up(rx, ry);
        rt.frame(0.0);
        assert_eq!((rt.state.clicks, rt.state.resets), (2, 0));
    }

    #[test]
    fn the_room_is_drawn_from_every_heading() {
        let mut s = Look::default();
        for k in 0..8 {
            s.yaw = k as f32 * TAU / 8.0;
            let shown = room().into_iter().filter(|&(a, b)| clip(&s, a, b).is_some()).count();
            assert!(shown > 20, "heading {k}: {shown} edges");
        }
    }
}
