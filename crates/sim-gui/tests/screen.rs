//! The widgets in egui without a window: the pointer as a finger, and keys
//! left to the app.

mod support;

use std::time::Duration;

use degus_sim::Sim;
use degus_sim_gui::egui::{
    self, Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, TouchDeviceId,
    TouchId, TouchPhase, vec2,
};
use degus_sim_gui::{Screen, SimThread, Zoom};
use support::*;

/// An egui without a window, showing a [`Screen`] and what else `extra`
/// shows, at 1:1 from (0, 0).
struct Window {
    ctx: Context,
    time: f64,
    focused: bool,
    screen: Screen,
    /// Where the screen is, after a frame.
    rect: Rect,
}

impl Window {
    fn new() -> Self {
        Self {
            ctx: Context::default(),
            time: 0.0,
            focused: true,
            screen: Screen::new(),
            rect: Rect::NOTHING,
        }
    }

    /// Runs a frame 20 ms after the last one, with `events`.
    fn frame(&mut self, sim: &SimThread, events: Vec<Event>) {
        self.frame_with(sim, events, |_| {});
    }

    fn frame_with(
        &mut self,
        sim: &SimThread,
        events: Vec<Event>,
        mut extra: impl FnMut(&mut egui::Ui),
    ) {
        self.time += 0.02;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            time: Some(self.time),
            focused: self.focused,
            events,
            ..RawInput::default()
        };
        let screen = &mut self.screen;
        let mut rect = Rect::NOTHING;
        let mut output = self.ctx.run_ui(input, |ui| {
            rect = screen.show(ui, sim, Zoom::Scale(1.0)).response.rect;
            extra(ui);
        });
        // Nothing to paint them on.
        output.textures_delta.clear();
        self.rect = rect;
    }

    /// A page pixel's place in the window.
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.rect.min + vec2(x, y)
    }
}

fn moved(pos: Pos2) -> Event {
    Event::PointerMoved(pos)
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn pressed(sim: &SimThread) -> bool {
    sim.lock().is_pressed()
}

#[test]
fn the_pointer_presses_moves_and_releases() {
    let sim = SimThread::new(Sim::new(panel()));
    let host = Host::new(&sim);
    let mut window = Window::new();
    window.frame(&sim, vec![]);
    let key = window.at(20.0, 20.0);
    window.frame(&sim, vec![moved(key), button(key, true)]);
    assert!(pressed(&sim));
    assert_eq!(host.frame(ANSWER), upload(KEY_VP, &[KEY_VALUE]));
    let tp = |sim: &SimThread| sim.lock().vp().read(0x0017, 2).unwrap().to_vec();
    assert_eq!(tp(&sim), [20, 20]);

    let elsewhere = window.at(30.0, 25.0);
    window.frame(&sim, vec![moved(elsewhere)]);
    assert!(pressed(&sim));
    assert_eq!(tp(&sim), [30, 25]);
    window.frame(&sim, vec![button(elsewhere, false)]);
    assert!(!pressed(&sim));
}

#[test]
fn a_click_between_two_frames_is_a_tap() {
    let sim = SimThread::new(Sim::new(panel()));
    let host = Host::new(&sim);
    let mut window = Window::new();
    window.frame(&sim, vec![]);
    let key = window.at(20.0, 20.0);
    window.frame(
        &sim,
        vec![moved(key), button(key, true), button(key, false)],
    );
    assert_eq!(host.frame(ANSWER), upload(KEY_VP, &[KEY_VALUE]));
    assert!(!pressed(&sim));
}

#[test]
fn the_touch_ends_when_the_pointer_leaves_the_screen() {
    let sim = SimThread::new(Sim::new(panel()));
    let mut window = Window::new();
    window.frame(&sim, vec![]);
    let key = window.at(20.0, 20.0);
    window.frame(&sim, vec![moved(key), button(key, true)]);
    assert!(pressed(&sim));
    let outside = window.at(250.0, 20.0);
    window.frame(&sim, vec![moved(outside)]);
    assert!(!pressed(&sim));
    // Back on the screen, the pointer still down touches nothing.
    window.frame(&sim, vec![moved(key)]);
    assert!(!pressed(&sim));
    window.frame(&sim, vec![button(key, false)]);
    window.frame(&sim, vec![button(key, true)]);
    assert!(pressed(&sim));

    // Off the window.
    window.frame(&sim, vec![Event::PointerGone]);
    assert!(!pressed(&sim));
}

#[test]
fn the_touch_ends_when_the_window_loses_focus() {
    let sim = SimThread::new(Sim::new(panel()));
    let mut window = Window::new();
    window.frame(&sim, vec![]);
    let key = window.at(20.0, 20.0);
    window.frame(&sim, vec![moved(key), button(key, true)]);
    assert!(pressed(&sim));
    window.focused = false;
    window.frame(&sim, vec![Event::WindowFocused(false)]);
    assert!(!pressed(&sim));
    window.focused = true;
    window.frame(&sim, vec![Event::WindowFocused(true)]);
    assert!(!pressed(&sim));
}

#[test]
fn the_touch_ends_soon_after_the_screen_is_not_shown() {
    let sim = SimThread::new(Sim::new(panel()));
    let mut window = Window::new();
    window.frame(&sim, vec![]);
    let key = window.at(20.0, 20.0);
    window.frame(&sim, vec![moved(key), button(key, true)]);
    // Held, and shown.
    for _ in 0..40 {
        window.frame(&sim, vec![]);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(pressed(&sim));
    // Not shown, as when the window is minimised.
    eventually("the touch ends", || !pressed(&sim));
    // Shown again, the pointer still down: no touch.
    window.frame(&sim, vec![moved(key)]);
    assert!(!pressed(&sim));
}

#[test]
fn a_finger_held_still_stays_down() {
    let sim = SimThread::new(Sim::new(panel()));
    let mut window = Window::new();
    window.frame(&sim, vec![]);
    let key = window.at(20.0, 20.0);
    let touch = |phase| Event::Touch {
        device_id: TouchDeviceId(1),
        id: TouchId(1),
        phase,
        pos: key,
        force: None,
    };
    window.frame(
        &sim,
        vec![touch(TouchPhase::Start), moved(key), button(key, true)],
    );
    assert!(pressed(&sim));
    // egui calls this a long touch after 0.8 s.
    for _ in 0..100 {
        window.frame(&sim, vec![]);
    }
    assert!(pressed(&sim));
    window.frame(
        &sim,
        vec![
            touch(TouchPhase::End),
            button(key, false),
            Event::PointerGone,
        ],
    );
    assert!(!pressed(&sim));
}

fn key(key: Key, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

#[test]
fn tab_and_space_never_reach_the_screen() {
    let sim = SimThread::new(Sim::new(panel()));
    let mut window = Window::new();
    window.frame(&sim, vec![]);
    for _ in 0..5 {
        let keys = vec![key(Key::Tab, true), key(Key::Tab, false)];
        window.frame(&sim, keys);
        assert_eq!(window.ctx.memory(|m| m.focused()), None);
        let keys = vec![key(Key::Space, true), key(Key::Space, false)];
        window.frame(&sim, keys);
    }
    assert!(!pressed(&sim));
}

#[test]
fn a_touch_takes_the_focus_from_a_text_field() {
    let sim = SimThread::new(Sim::new(panel()));
    let mut window = Window::new();
    let mut text = String::new();
    let mut field = |ui: &mut egui::Ui| {
        ui.text_edit_singleline(&mut text);
    };
    window.frame_with(&sim, vec![], &mut field);
    let keys = vec![key(Key::Tab, true), key(Key::Tab, false)];
    window.frame_with(&sim, keys, &mut field);
    assert!(window.ctx.memory(|m| m.focused()).is_some());

    // Holding the key down, longer than a click.
    let at = window.at(20.0, 20.0);
    window.frame_with(&sim, vec![moved(at), button(at, true)], &mut field);
    assert!(pressed(&sim));
    assert_eq!(window.ctx.memory(|m| m.focused()), None);
    let held = vec![key(Key::Space, true)];
    window.frame_with(&sim, held, &mut field);
    assert!(window.ctx.input(|i| i.key_down(Key::Space)));
    assert!(pressed(&sim));
    drop(field);
    assert_eq!(text, "");
}

#[test]
fn fits_the_screen_to_the_space() {
    let sim = SimThread::new(Sim::new(panel()));
    let ctx = Context::default();
    let mut screen = Screen::new();
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 600.0))),
        ..RawInput::default()
    };
    let mut size = vec2(0.0, 0.0);
    let mut output = ctx.run_ui(input, |ui| {
        size = screen.show(ui, &sim, Zoom::Fit).response.rect.size();
    });
    output.textures_delta.clear();
    // 200 × 120 at 2:1, limited by the width.
    assert_eq!(size, vec2(400.0, 240.0));
}
