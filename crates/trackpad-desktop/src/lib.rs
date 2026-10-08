//! `trackpad-desktop`: networking + OS input injection.
//!
//! Phase 1: OS-agnostic [`InputInjector`] trait, [`MockInjector`] for tests,
//! real [`WindowsInjector`] via `SendInput`, and [`service`] — a minimal UDP
//! loop (decode → inject, PING → PONG). Pairing, encryption, discovery and
//! the macOS injector land in later phases.

pub mod discovery;
pub mod pairing_client;
pub mod pairing_server;
pub mod qr;
pub mod service;
pub mod trust;
pub mod web;

use trackpad_core::{button, Message};

/// Abstracts OS cursor/button/scroll injection.
///
/// Everything above this trait stays OS-agnostic. The real injectors
/// implement this with native APIs; tests use [`MockInjector`].
pub trait InputInjector {
    /// Move the cursor relatively by (`dx`, `dy`) pixels.
    fn move_relative(&mut self, dx: i32, dy: i32);
    /// Inject a left click (down + up).
    fn click_left(&mut self);
    /// Inject a right click (down + up).
    fn click_right(&mut self);
    /// Inject a middle click (down + up). Windows does this with
    /// BUTTON_DOWN/UP pairs; mock counts them for tests.
    fn click_middle(&mut self);
    /// Scroll by (`dx`, `dy`) finger pixels. Positive `dy` scrolls content
    /// down in natural mode (sender already applied the natural toggle).
    fn scroll(&mut self, dx: i32, dy: i32);
    /// Press and hold a button (`button::LEFT/RIGHT/MIDDLE`).
    fn button_down(&mut self, button: u8);
    /// Release a held button. Unknown ids are ignored.
    fn button_up(&mut self, button: u8);

    /// Test hook: the mock injector returns itself for assertions.
    /// Real injectors return `None`.
    fn as_mock(&mut self) -> Option<&mut MockInjector> {
        None
    }
}

/// Test injector that records calls instead of touching the OS.
#[derive(Debug, Default)]
pub struct MockInjector {
    /// Accumulated relative movement since creation.
    pub moved: (i32, i32),
    /// Accumulated scroll pixels since creation.
    pub scrolled: (i32, i32),
    /// Number of left clicks injected.
    pub clicks: u32,
    /// Number of right clicks injected.
    pub right_clicks: u32,
    /// Number of middle clicks injected.
    pub middle_clicks: u32,
    /// Currently held buttons (bitmask over button ids 0..8).
    pub held: u8,
    /// Total down / up events (including redundant repeats).
    pub downs: u32,
    pub ups: u32,
}

impl InputInjector for MockInjector {
    fn move_relative(&mut self, dx: i32, dy: i32) {
        self.moved.0 += dx;
        self.moved.1 += dy;
    }

    fn click_left(&mut self) {
        self.clicks += 1;
    }

    fn click_right(&mut self) {
        self.right_clicks += 1;
    }

    fn click_middle(&mut self) {
        self.middle_clicks += 1;
    }

    fn scroll(&mut self, dx: i32, dy: i32) {
        self.scrolled.0 += dx;
        self.scrolled.1 += dy;
    }

    fn button_down(&mut self, button: u8) {
        if button < 8 {
            self.held |= 1 << button;
        }
        self.downs += 1;
    }

    fn button_up(&mut self, button: u8) {
        if button < 8 {
            self.held &= !(1 << button);
        }
        self.ups += 1;
    }

    fn as_mock(&mut self) -> Option<&mut MockInjector> {
        Some(self)
    }
}

/// Apply one decoded [`Message`] to an injector. Unknown buttons are
/// ignored (forward-compat for future button ids).
pub fn dispatch(msg: Message, injector: &mut (dyn InputInjector + '_)) {
    match msg {
        Message::Move { dx, dy } => injector.move_relative(dx as i32, dy as i32),
        Message::Click { button: b } => match b {
            button::LEFT => injector.click_left(),
            button::RIGHT => injector.click_right(),
            button::MIDDLE => injector.click_middle(),
            _ => {}
        },
        Message::ButtonDown { button: b } => injector.button_down(b),
        Message::ButtonUp { button: b } => injector.button_up(b),
        Message::Scroll { dx, dy } => injector.scroll(dx as i32, dy as i32),
        Message::Ping { .. } | Message::Pong { .. } => {}
    }
}

/// Real injector for Windows 10/11 using `SendInput`.
///
/// Relative moves use `MOUSEEVENTF_MOVE` (no `ABSOLUTE` flag), so DPI
/// scaling and multi-monitor work as the OS maps relative deltas itself.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsInjector {
    scroll_acc: ScrollAccum,
}

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{MOUSEEVENTF_ABSOLUTE, MOUSE_EVENT_FLAGS};

#[cfg(windows)]
impl WindowsInjector {
    /// Send one raw `INPUT` mouse event. Returns an error instead of
    /// panicking when the OS drops the event (e.g. UIPI block).
    fn send_mouse(dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS) -> Result<(), std::io::Error> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT,
        };
        // Absolute flag must never be set: this app only sends relative moves.
        debug_assert_eq!((flags & MOUSEEVENTF_ABSOLUTE).0, 0);
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    /// Send a wheel event. `delta` is in WHEEL_DELTA units (120 per notch,
    /// negative for the opposite direction).
    fn send_wheel(
        delta: i32,
        flag: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
    ) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT,
        };
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: delta as u32,
                    dwFlags: flag,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent != 1 {
            eprintln!(
                "trackpad-service: SendInput(wheel) failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

#[cfg(windows)]
impl InputInjector for WindowsInjector {
    fn move_relative(&mut self, dx: i32, dy: i32) {
        use windows::Win32::UI::Input::KeyboardAndMouse::MOUSEEVENTF_MOVE;
        if dx == 0 && dy == 0 {
            return;
        }
        if let Err(e) = Self::send_mouse(dx, dy, MOUSEEVENTF_MOVE) {
            eprintln!("trackpad-service: SendInput(move) failed: {e}");
        }
    }

    fn click_left(&mut self) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        };
        if let Err(e) = Self::send_mouse(0, 0, MOUSEEVENTF_LEFTDOWN) {
            eprintln!("trackpad-service: SendInput(left down) failed: {e}");
        }
        if let Err(e) = Self::send_mouse(0, 0, MOUSEEVENTF_LEFTUP) {
            eprintln!("trackpad-service: SendInput(left up) failed: {e}");
        }
    }

    fn click_right(&mut self) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        };
        if let Err(e) = Self::send_mouse(0, 0, MOUSEEVENTF_RIGHTDOWN) {
            eprintln!("trackpad-service: SendInput(right down) failed: {e}");
        }
        if let Err(e) = Self::send_mouse(0, 0, MOUSEEVENTF_RIGHTUP) {
            eprintln!("trackpad-service: SendInput(right up) failed: {e}");
        }
    }

    fn click_middle(&mut self) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
        };
        if let Err(e) = Self::send_mouse(0, 0, MOUSEEVENTF_MIDDLEDOWN) {
            eprintln!("trackpad-service: SendInput(middle down) failed: {e}");
        }
        if let Err(e) = Self::send_mouse(0, 0, MOUSEEVENTF_MIDDLEUP) {
            eprintln!("trackpad-service: SendInput(middle up) failed: {e}");
        }
    }

    fn scroll(&mut self, dx: i32, dy: i32) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL};
        let (wx, wy) = self.scroll_acc.push(dx, dy);
        if wx != 0 {
            Self::send_wheel(wx, MOUSEEVENTF_HWHEEL);
        }
        if wy != 0 {
            Self::send_wheel(wy, MOUSEEVENTF_WHEEL);
        }
    }

    fn button_down(&mut self, b: u8) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_RIGHTDOWN,
        };
        let flag = match b {
            button::LEFT => MOUSEEVENTF_LEFTDOWN,
            button::RIGHT => MOUSEEVENTF_RIGHTDOWN,
            button::MIDDLE => MOUSEEVENTF_MIDDLEDOWN,
            _ => return,
        };
        if let Err(e) = Self::send_mouse(0, 0, flag) {
            eprintln!("trackpad-service: SendInput(button {b} down) failed: {e}");
        }
    }

    fn button_up(&mut self, b: u8) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTUP,
        };
        let flag = match b {
            button::LEFT => MOUSEEVENTF_LEFTUP,
            button::RIGHT => MOUSEEVENTF_RIGHTUP,
            button::MIDDLE => MOUSEEVENTF_MIDDLEUP,
            _ => return,
        };
        if let Err(e) = Self::send_mouse(0, 0, flag) {
            eprintln!("trackpad-service: SendInput(button {b} up) failed: {e}");
        }
    }
}

/// Finger pixels per wheel notch (WHEEL_DELTA = 120). Two-finger scroll
/// accumulates here so slow drags still emit smooth wheel events.
pub const SCROLL_NOTCH_PX: i32 = 40;

/// Converts sender-space finger pixels into wheel deltas. Pure and
/// platform-independent; the OS injector owns one and emits what it returns.
#[derive(Debug, Default)]
pub struct ScrollAccum {
    x: i32,
    y: i32,
}

impl ScrollAccum {
    /// Feed finger pixels (`dy > 0` = fingers moved down, natural mode
    /// already applied by the sender). Returns `(horizontal, vertical)`
    /// wheel deltas in WHEEL_DELTA units (120 per notch) to emit now.
    pub fn push(&mut self, dx: i32, dy: i32) -> (i32, i32) {
        self.x += dx;
        self.y += dy;
        let wx = self.x / SCROLL_NOTCH_PX * 120;
        let wy = self.y / SCROLL_NOTCH_PX * 120;
        self.x %= SCROLL_NOTCH_PX;
        self.y %= SCROLL_NOTCH_PX;
        (wx, wy)
    }
}

/// Milliseconds since the Unix epoch (service clock).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Phase 0 smoke string proving desktop links against `trackpad-core`.
pub fn hello() -> String {
    format!("trackpad-desktop hello via {}", trackpad_core::hello())
}

#[cfg(test)]
mod tests {
    use super::*;
    use trackpad_core::BUTTON_LEFT;

    #[test]
    fn mock_injector_records_moves_and_clicks() {
        let mut m = MockInjector::default();
        m.move_relative(10, -5);
        m.move_relative(-3, 2);
        m.click_left();
        assert_eq!(m.moved, (7, -3));
        assert_eq!(m.clicks, 1);
    }

    #[test]
    fn dispatch_routes_all_input_messages() {
        let mut m = MockInjector::default();
        dispatch(Message::Move { dx: 4, dy: -2 }, &mut m);
        dispatch(
            Message::Click {
                button: BUTTON_LEFT,
            },
            &mut m,
        );
        dispatch(Message::Click { button: 1 }, &mut m);
        dispatch(
            Message::Click {
                button: button::MIDDLE,
            },
            &mut m,
        );
        dispatch(Message::Click { button: 99 }, &mut m);
        dispatch(Message::Scroll { dx: 10, dy: -80 }, &mut m);
        dispatch(Message::ButtonDown { button: 0 }, &mut m);
        dispatch(Message::ButtonUp { button: 0 }, &mut m);
        dispatch(Message::Ping { timestamp_ms: 1 }, &mut m);
        assert_eq!(m.moved, (4, -2));
        assert_eq!(m.clicks, 1);
        assert_eq!(m.right_clicks, 1);
        assert_eq!(m.middle_clicks, 1);
        assert_eq!(m.scrolled, (10, -80));
        assert_eq!(m.held, 0);
        assert_eq!((m.downs, m.ups), (1, 1));
    }

    #[test]
    fn scroll_accum_emits_notches_and_keeps_remainder() {
        let mut acc = ScrollAccum::default();
        assert_eq!(acc.push(10, 10), (0, 0));
        assert_eq!(acc.push(30, 30), (120, 120));
        assert_eq!(acc.push(-80, 20), (-240, 0));
        assert_eq!(acc.push(-5, 15), (0, 0));
    }

    #[test]
    fn hello_links_core() {
        assert!(hello().contains("trackpad-core hello"));
    }
}
