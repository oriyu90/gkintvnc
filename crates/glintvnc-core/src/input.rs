//! X11 keysyms required by classic RFB KeyEvent; mapping is explicit and testable.
use crate::configuration::KeyPreset;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Character(char),
    Return,
    Escape,
    Backspace,
    Tab,
    Delete,
    Insert,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Shift,
    Control,
    Alt,
    Command,
    CapsLock,
    NumLock,
    ScrollLock,
    PrintScreen,
    Pause,
    Menu,
}

pub fn keysym(key: Key, preset: &KeyPreset) -> Option<u32> {
    match key {
        Key::Character(c) if (0x20..=0x7e).contains(&(c as u32)) => Some(c as u32),
        Key::Character(c) => Some(0x0100_0000 | c as u32), // Unicode keysym: server support varies
        Key::Return => Some(0xff0d),
        Key::Escape => Some(0xff1b),
        Key::Backspace => Some(0xff08),
        Key::Tab => Some(0xff09),
        Key::Delete => Some(0xffff),
        Key::Insert => Some(0xff63),
        Key::Space => Some(0xff20),
        Key::Up => Some(0xff52),
        Key::Down => Some(0xff54),
        Key::Left => Some(0xff51),
        Key::Right => Some(0xff53),
        Key::Home => Some(0xff50),
        Key::End => Some(0xff57),
        Key::PageUp => Some(0xff55),
        Key::PageDown => Some(0xff56),
        Key::F(i) if (1..=35).contains(&i) => Some(0xffbd + u32::from(i)),
        Key::F(_) => None,
        Key::CapsLock => Some(0xffe5),
        Key::NumLock => Some(0xff7f),
        Key::ScrollLock => Some(0xff14),
        Key::PrintScreen => Some(0xff61),
        Key::Pause => Some(0xff13),
        Key::Menu => Some(0xff67),
        Key::Shift => Some(0xffe1),
        Key::Control => Some(match preset {
            KeyPreset::Identity => 0xffe3,
            _ => 0xffeb,
        }),
        Key::Alt => Some(0xffe9),
        Key::Command => Some(match preset {
            KeyPreset::MacToLinux | KeyPreset::MacToWindows => 0xffe3,
            KeyPreset::Identity => 0xffeb,
        }),
    }
}

pub fn mapped_modifiers(
    preset: &KeyPreset,
    mac_command: bool,
    mac_option: bool,
    mac_control: bool,
) -> [bool; 3] {
    // [X11 ctrl, alt, super]; useful to build keydown/up transitions after egui text integration.
    match preset {
        KeyPreset::MacToWindows | KeyPreset::MacToLinux => [mac_command, mac_option, mac_control],
        KeyPreset::Identity => [mac_control, mac_option, mac_command],
    }
}

/// Full button mask for one RFB wheel pulse: press `wheel_bit` while preserving
/// whatever physical buttons the user is already holding, then restore them.
/// Pure so the drag+scroll regression can be unit-tested.
pub fn scroll_pulse_buttons(held: u8, wheel_bit: u8) -> (u8, u8) {
    (held | wheel_bit, held)
}

/// 16ms pointer-motion throttle with a latest-wins pending slot.
///
/// Matches the de-facto viewer standard (TigerVNC `PointerEventInterval=17`,
/// noVNC `MOUSE_MOVE_DELAY=17`) at our 60Hz refresh: pure motion is sent at most
/// once per interval, always the newest position; button-mask changes are never
/// throttled; stationary positions are never re-sent; pressing/releasing a button
/// flushes the pending move first to keep message order. Implemented as a pure
/// state machine over `now_ms` so it is unit-testable.
#[derive(Default, Debug, Clone)]
pub struct MotionThrottle {
    last_sent: Option<(u16, u16, u8)>,
    last_sent_ms: Option<u64>,
    pending: Option<(u16, u16, u8)>,
}

impl MotionThrottle {
    pub const INTERVAL_MS: u64 = 16;

    pub fn new() -> Self {
        Self::default()
    }

    fn note(&mut self, pos: (u16, u16, u8), now_ms: u64) {
        self.last_sent = Some(pos);
        self.last_sent_ms = Some(now_ms);
        self.pending = None;
    }

    /// Record an out-of-band send (button press/release edge). Keeps the
    /// throttle's view of the wire in sync with reality.
    pub fn note_sent(&mut self, pos: (u16, u16, u8), now_ms: u64) {
        self.note(pos, now_ms);
    }

    /// Decide what to do with a fresh motion sample. Returns the position to
    /// send, or None (folded into the pending slot or suppressed).
    pub fn on_motion(&mut self, pos: (u16, u16, u8), now_ms: u64) -> Option<(u16, u16, u8)> {
        if self.last_sent == Some(pos) {
            // Position unchanged since last send: never re-send.
            return None;
        }
        let due = self
            .last_sent_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= Self::INTERVAL_MS);
        let mask_changed = self.last_sent.map(|p| p.2) != Some(pos.2);
        if due || mask_changed {
            self.note(pos, now_ms);
            return Some(pos);
        }
        self.pending = Some(pos);
        None
    }

    /// Send the pending move if its interval has elapsed.
    pub fn flush_if_due(&mut self, now_ms: u64) -> Option<(u16, u16, u8)> {
        let p = self.pending?;
        if self.last_sent == Some(p) {
            self.pending = None;
            return None;
        }
        let due = self
            .last_sent_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= Self::INTERVAL_MS);
        if due {
            self.note(p, now_ms);
            return Some(p);
        }
        None
    }

    /// Force-send the pending move immediately (used before button edges so
    /// ordering is preserved).
    pub fn flush_pending(&mut self, now_ms: u64) -> Option<(u16, u16, u8)> {
        let p = self.pending.take()?;
        if self.last_sent == Some(p) {
            return None;
        }
        self.note(p, now_ms);
        Some(p)
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scroll_pulse_preserves_held_buttons() {
        assert_eq!(super::scroll_pulse_buttons(0, 8), (8, 0));
        assert_eq!(super::scroll_pulse_buttons(1, 16), (17, 1));
        assert_eq!(super::scroll_pulse_buttons(0b101, 64), (0b100_0101, 0b101));
    }

    #[test]
    fn motion_throttle_coalesces_and_flushes() {
        let mut t = MotionThrottle::new();
        // First motion goes out immediately.
        assert_eq!(t.on_motion((10, 10, 0), 0), Some((10, 10, 0)));
        // Rapid follow-ups fold into pending; nothing sent yet.
        assert_eq!(t.on_motion((11, 10, 0), 4), None);
        assert_eq!(t.on_motion((12, 10, 0), 8), None);
        assert!(t.has_pending());
        assert_eq!(t.flush_if_due(10), None);
        assert_eq!(t.flush_if_due(16), Some((12, 10, 0)));
        assert!(!t.has_pending());
    }

    #[test]
    fn motion_throttle_sends_mask_changes_immediately() {
        let mut t = MotionThrottle::new();
        assert_eq!(t.on_motion((10, 10, 0), 0), Some((10, 10, 0)));
        // 1ms later but button mask changed: immediate, throttling bypassed.
        assert_eq!(t.on_motion((10, 10, 1), 1), Some((10, 10, 1)));
        // Stationary position is never re-sent even after the interval.
        assert_eq!(t.on_motion((10, 10, 1), 100), None);
        assert_eq!(t.flush_if_due(100), None);
    }

    #[test]
    fn motion_throttle_flush_pending_preserves_order() {
        let mut t = MotionThrottle::new();
        assert_eq!(t.on_motion((1, 1, 1), 0), Some((1, 1, 1)));
        assert_eq!(t.on_motion((2, 2, 1), 2), None); // mask unchanged, within 16ms -> pending
                                                     // Edge arrives before interval elapsed: pending move must go first.
        assert_eq!(t.flush_pending(3), Some((2, 2, 1)));
        assert_eq!(t.flush_pending(3), None);
    }

    #[test]
    fn motion_throttle_note_sent_keeps_sync() {
        let mut t = MotionThrottle::new();
        t.note_sent((7, 8, 1), 0);
        // Same position+mask right after an edge: suppressed.
        assert_eq!(t.on_motion((7, 8, 1), 1), None);
        assert_eq!(t.on_motion((9, 8, 1), 1), None); // throttled into pending
        assert_eq!(t.flush_pending(2), Some((9, 8, 1)));
    }

    use super::*;
    #[test]
    fn unicode_keysym() {
        assert_eq!(
            keysym(Key::Character('あ'), &KeyPreset::Identity),
            Some(0x01003042)
        );
    }
    #[test]
    fn return_key() {
        assert_eq!(keysym(Key::Return, &KeyPreset::Identity), Some(0xff0d));
    }
    #[test]
    fn lock_and_system_keys() {
        assert_eq!(keysym(Key::CapsLock, &KeyPreset::Identity), Some(0xffe5));
        assert_eq!(keysym(Key::NumLock, &KeyPreset::Identity), Some(0xff7f));
        assert_eq!(keysym(Key::ScrollLock, &KeyPreset::Identity), Some(0xff14));
        assert_eq!(keysym(Key::PrintScreen, &KeyPreset::Identity), Some(0xff61));
        assert_eq!(keysym(Key::Pause, &KeyPreset::Identity), Some(0xff13));
        assert_eq!(keysym(Key::Menu, &KeyPreset::Identity), Some(0xff67));
        assert_eq!(keysym(Key::Insert, &KeyPreset::Identity), Some(0xff63));
        assert_eq!(keysym(Key::Space, &KeyPreset::Identity), Some(0xff20));
    }
    #[test]
    fn function_keys_extend_to_f35() {
        assert_eq!(keysym(Key::F(1), &KeyPreset::Identity), Some(0xffbe));
        assert_eq!(keysym(Key::F(12), &KeyPreset::Identity), Some(0xffc9));
        assert_eq!(keysym(Key::F(35), &KeyPreset::Identity), Some(0xffe0));
        assert_eq!(keysym(Key::F(36), &KeyPreset::Identity), None);
    }
}
