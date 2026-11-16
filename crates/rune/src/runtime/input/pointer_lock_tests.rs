use super::*;
use crate::el::{col, el};
use crate::{Key, KeyEvent, Mods, PhysicalInput, PhysicalKeyCode};

fn lock_step(state: &mut LockState, event: LockEvent) -> Vec<LockEffect> { step(state, event).into_iter().collect() }

#[test]
fn the_machine_grabs_on_a_press_and_lets_go_as_its_mode_says() {
    let mut state = LockState::Off;
    let press = |release| LockEvent::Pressed { path: "view", release, at: (5.0, 6.0) };
    assert_eq!(lock_step(&mut state, LockEvent::ButtonUp), [], "nothing held");
    assert_eq!(lock_step(&mut state, press(PointerLockRelease::Escape)), [LockEffect::Grab]);
    assert_eq!(lock_step(&mut state, press(PointerLockRelease::Escape)), [], "a second press takes nothing more");
    assert_eq!(lock_step(&mut state, LockEvent::ButtonUp), [], "an Escape lock outlives the release");
    assert_eq!(lock_step(&mut state, LockEvent::Asked(Some(PointerLockRelease::Escape))), []);
    assert_eq!(lock_step(&mut state, LockEvent::Released), [LockEffect::Ungrab]);
    assert_eq!(state, LockState::Off);

    assert_eq!(lock_step(&mut state, press(PointerLockRelease::Release)), [LockEffect::Grab]);
    assert_eq!(lock_step(&mut state, LockEvent::ButtonUp), [LockEffect::Ungrab], "a held lock ends with the button");
    assert_eq!(lock_step(&mut state, press(PointerLockRelease::Escape)), [LockEffect::Grab]);
    assert_eq!(lock_step(&mut state, LockEvent::Asked(None)), [LockEffect::Ungrab], "the element stopped asking");
    assert_eq!(state, LockState::Off);
}

#[test]
fn opposite_effects_owed_to_the_window_cancel_out() {
    let mut lock = PointerLock::default();
    lock.step(LockEvent::Pressed { path: "view", release: PointerLockRelease::Escape, at: (0.0, 0.0) });
    assert_eq!(lock.owed, Some(LockEffect::Grab));
    lock.step(LockEvent::Released);
    assert_eq!(lock.owed, None, "a grab the window never made needs no release");
}

#[derive(Default)]
struct Game {
    playing: bool,
    turned: Vec<[f32; 2]>,
    presses: Vec<&'static str>,
    keys: Vec<Key>,
    locked_seen: bool,
}

/// A 40-pt toolbar over a 300×160 viewport that asks for the lock while playing, with a HUD
/// button inside it and a card drawn over its bottom-right corner.
fn game(s: &Game, _: &mut Cx<Game>) -> El {
    let mut view = el().key("view").size(300.0, 160.0).on_press(|s: &mut Game| s.presses.push("view"))
        .child(el().key("hud").size(40.0, 20.0).on_press(|s: &mut Game| s.presses.push("hud")));
    if s.playing { view = view.pointer_lock(|s: &mut Game, d, _| s.turned.push(d)); }
    let card = el().key("card").size(60.0, 40.0).pin(None, None, Some(0.0), Some(0.0)).on_press(|s: &mut Game| s.presses.push("card"));
    col().size(300.0, 200.0)
        .child(el().key("toolbar").size(300.0, 40.0).on_press(|s: &mut Game| s.presses.push("toolbar")))
        .child(view)
        .child(card)
}

fn playing() -> Runtime<Game> {
    let mut rt = Runtime::new(Game { playing: true, ..Default::default() }, game, (300.0, 200.0), Env::new(true, 1.0));
    rt.on_key = Some(|s, ev, _| { s.keys.push(ev.key.clone()); true });
    rt.window_focus(true);
    rt.frame(0.0);
    rt
}

fn click(rt: &mut Runtime<Game>, x: f32, y: f32) {
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
}

#[test]
fn a_press_on_the_view_captures_and_motion_reaches_it() {
    let mut rt = playing();
    rt.pointer_motion(5.0, 5.0);
    rt.frame(0.0);
    assert!(rt.state.turned.is_empty(), "motion before the capture is dropped");
    click(&mut rt, 150.0, 10.0);
    assert!(!rt.pointer_locked(), "the toolbar doesn't capture");
    click(&mut rt, 150.0, 100.0);
    assert!(rt.pointer_locked());
    assert_eq!(rt.state.presses, ["toolbar", "view"], "the capturing press still presses");
    assert_eq!(rt.take_pointer_lock_effect(), Some(LockEffect::Grab));
    assert_eq!(rt.take_pointer_lock_effect(), None, "owed once");
    rt.pointer_motion(12.0, -3.0);
    rt.pointer_motion(0.5, 0.0);
    rt.pointer_motion(0.0, 0.0);
    rt.frame(0.0);
    assert_eq!(rt.state.turned, [[12.5, -3.0]], "motion between frames adds up");
}

#[test]
fn a_press_on_a_covering_card_or_a_control_inside_doesnt_capture() {
    let mut rt = playing();
    click(&mut rt, 270.0, 180.0);
    click(&mut rt, 10.0, 50.0);
    assert!(!rt.pointer_locked());
    assert_eq!(rt.state.presses, ["card", "hud"]);
}

#[test]
fn while_captured_the_pointer_stays_where_the_press_took_it() {
    let mut rt = playing();
    click(&mut rt, 150.0, 100.0);
    rt.pointer_move(10.0, 10.0);
    rt.pointer_leave();
    assert_eq!(rt.pointer(), Some((150.0, 100.0)));
    assert_eq!(rt.cursor(), crate::el::Cursor::Hidden);
    rt.pointer_down(150.0, 10.0);
    rt.pointer_up(150.0, 10.0);
    rt.frame(0.0);
    assert_eq!(rt.state.presses, ["view", "view"], "a click over the toolbar lands on the view");
    rt.window_focus(false);
    rt.frame(0.0);
    assert!(!rt.pointer_locked());
    assert_eq!(rt.take_pointer_lock_effect(), None, "the grab was never made");
    rt.window_focus(true);
    rt.pointer_move(10.0, 10.0);
    assert_eq!(rt.pointer(), Some((10.0, 10.0)), "moves work again, and refocus doesn't capture");
    assert!(!rt.pointer_locked());
}

#[test]
fn many_motions_before_a_frame_queue_once() {
    let mut rt = playing();
    click(&mut rt, 150.0, 100.0);
    for _ in 0..8 { rt.pointer_motion(1.5, -0.25); }
    assert_eq!(rt.queue.iter().filter(|a| matches!(a, Action::PointerLockMotion(..))).count(), 1);
    rt.frame(0.0);
    assert_eq!(rt.state.turned, [[12.0, -2.0]]);
}

#[test]
fn escape_lets_go_and_goes_no_further() {
    let mut rt = playing();
    click(&mut rt, 150.0, 100.0);
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    rt.frame(0.0);
    assert!(!rt.pointer_locked());
    assert!(rt.state.keys.is_empty(), "the app doesn't hear the Escape that let go");
    assert_eq!(rt.take_pointer_lock_effect(), None, "the grab was never made");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    rt.frame(0.0);
    assert_eq!(rt.state.keys, [Key::Escape], "a second Escape reaches the app");
    rt.pointer_motion(3.0, 3.0);
    rt.frame(0.0);
    assert!(rt.state.turned.is_empty(), "motion after the release is dropped");
    // The physical key path lets go the same way.
    click(&mut rt, 150.0, 100.0);
    rt.physical_input(PhysicalInput::Key { key: PhysicalKeyCode::Escape, mods: Mods::default(), pressed: true, repeat: false });
    assert!(!rt.pointer_locked());
}

#[test]
fn focus_loss_and_the_element_going_away_let_go_with_a_release_owed() {
    let mut rt = playing();
    click(&mut rt, 150.0, 100.0);
    assert_eq!(rt.take_pointer_lock_effect(), Some(LockEffect::Grab));
    rt.window_focus(false);
    assert_eq!(rt.take_pointer_lock_effect(), Some(LockEffect::Ungrab));
    rt.window_focus(true);
    rt.frame(0.0);
    assert!(!rt.pointer_locked(), "refocus doesn't capture");

    click(&mut rt, 150.0, 100.0);
    assert_eq!(rt.take_pointer_lock_effect(), Some(LockEffect::Grab));
    rt.update(|s, _| s.playing = false);
    rt.frame(0.0);
    assert!(!rt.pointer_locked());
    assert_eq!(rt.take_pointer_lock_effect(), Some(LockEffect::Ungrab));
    click(&mut rt, 150.0, 100.0);
    assert!(!rt.pointer_locked(), "it no longer asks");
}

#[test]
fn the_view_hears_whether_the_pointer_is_locked() {
    fn view(_: &Game, cx: &mut Cx<Game>) -> El {
        let locked = cx.pointer_locked();
        el().key("view").size(100.0, 100.0)
            .on_press(move |s: &mut Game| s.locked_seen = locked)
            .pointer_lock(|s: &mut Game, d, _| s.turned.push(d))
    }
    let mut rt = Runtime::new(Game::default(), view, (100.0, 100.0), Env::new(true, 1.0));
    rt.frame(0.0);
    rt.pointer_down(50.0, 50.0);
    rt.pointer_up(50.0, 50.0);
    rt.frame(0.0);
    rt.pointer_down(50.0, 50.0);
    rt.pointer_up(50.0, 50.0);
    rt.frame(0.0);
    assert!(rt.state.locked_seen, "the view re-ran with the lock held");
}

#[derive(Default)]
struct Knob {
    volume: f32,
    drags: Vec<(Phase, bool)>,
}

fn knobs(_: &Knob, _: &mut Cx<Knob>) -> El {
    let knob = |key: &'static str| el().key(key).size(40.0, 40.0)
        .on_drag(|s: &mut Knob, d, _| {
            s.drags.push((d.phase, d.cancelled()));
            match d.phase {
                Phase::Down => d.data = s.volume,
                Phase::Up if d.cancelled() => s.volume = d.data,
                _ => {}
            }
        })
        .pointer_lock_while_held(|s: &mut Knob, [_, dy], _| s.volume = (s.volume - dy / 100.0).clamp(0.0, 1.0));
    crate::el::row().size(200.0, 40.0).child(knob("a")).child(knob("b"))
}

fn knob_runtime() -> Runtime<Knob> {
    let mut rt = Runtime::new(Knob { volume: 0.5, ..Default::default() }, knobs, (200.0, 40.0), Env::new(true, 1.0));
    rt.window_focus(true);
    rt.frame(0.0);
    rt
}

#[test]
fn a_held_lock_turns_past_the_screen_and_lets_go_on_release() {
    let mut rt = knob_runtime();
    rt.pointer_move(20.0, 20.0);
    rt.pointer_down(20.0, 20.0);
    rt.frame(0.0);
    assert!(rt.pointer_locked());
    // Far more travel than any screen has room for below the knob.
    for _ in 0..30 { rt.pointer_motion(0.0, 5.0); rt.frame(0.0); }
    assert_eq!(rt.state.volume, 0.0, "turned all the way down");
    // The OS cursor never moved, so the release comes where it was pressed.
    rt.pointer_up(20.0, 20.0);
    rt.frame(0.0);
    assert!(!rt.pointer_locked(), "the release lets go");
    assert_eq!(rt.take_pointer_lock_effect(), None, "grabbed and let go before the window looked");
    assert_eq!(rt.state.drags, [(Phase::Down, false), (Phase::Up, false)], "no Move while held");
    rt.pointer_motion(0.0, -50.0);
    rt.frame(0.0);
    assert_eq!(rt.state.volume, 0.0, "motion after the release is dropped");
    rt.pointer_move(30.0, 30.0);
    assert_eq!(rt.pointer(), Some((30.0, 30.0)), "the pointer moves again");
}

#[test]
fn any_of_several_knobs_takes_the_lock() {
    let mut rt = knob_runtime();
    rt.pointer_move(60.0, 20.0);
    rt.pointer_down(60.0, 20.0);
    rt.frame(0.0);
    assert!(rt.pointer_locked(), "the second knob, not only the frontmost asking element");
    rt.pointer_motion(0.0, -20.0);
    rt.frame(0.0);
    assert!((rt.state.volume - 0.7).abs() < 1e-5, "{}", rt.state.volume);
    rt.pointer_up(60.0, 20.0);
    assert!(!rt.pointer_locked());
}

#[test]
fn escape_during_a_held_lock_cancels_the_drag_and_lets_go() {
    let mut rt = knob_runtime();
    rt.pointer_move(20.0, 20.0);
    rt.pointer_down(20.0, 20.0);
    rt.frame(0.0);
    rt.pointer_motion(0.0, 30.0);
    rt.frame(0.0);
    assert!((rt.state.volume - 0.2).abs() < 1e-5);
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    rt.frame(0.0);
    assert!(!rt.pointer_locked());
    assert_eq!(rt.state.volume, 0.5, "the cancelled drag put the volume back");
    assert_eq!(rt.state.drags.last(), Some(&(Phase::Up, true)));
    rt.pointer_up(20.0, 20.0);
    rt.frame(0.0);
    assert_eq!(rt.state.drags.iter().filter(|d| d.0 == Phase::Up).count(), 1, "the release after it does nothing");
}

#[test]
fn a_host_that_cant_hold_the_pointer_never_captures() {
    let mut rt = knob_runtime();
    rt.set_os_pointer_lock(false);
    rt.pointer_move(20.0, 20.0);
    rt.pointer_down(20.0, 20.0);
    assert!(!rt.pointer_locked());
    rt.pointer_up(20.0, 20.0);
}

#[test]
fn a_finger_never_captures() {
    let mut rt = playing();
    rt.touch(1, crate::TouchPhase::Started, 150.0, 100.0, 0.0);
    assert!(!rt.pointer_locked());
    assert_eq!(rt.take_pointer_lock_effect(), None, "nothing grabbed");
    rt.touch(1, crate::TouchPhase::Ended, 150.0, 100.0, 0.05);
}
