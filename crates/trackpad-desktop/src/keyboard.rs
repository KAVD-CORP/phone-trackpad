//! Key-combo parser shared by the phone command and the Handsfree tools.
//!
//! The string is the wire form (`ctrl+enter`). Injection turns it into
//! `SendInput` on Windows. Canonical order is ctrl, alt, shift, win, then
//! the key, so tests can compare one spelling.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Space,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Letter(u8),
    Digit(u8),
    Function(u8),
}

impl Key {
    pub fn name(self) -> String {
        match self {
            Key::Enter => "enter".into(),
            Key::Tab => "tab".into(),
            Key::Escape => "escape".into(),
            Key::Backspace => "backspace".into(),
            Key::Delete => "delete".into(),
            Key::Space => "space".into(),
            Key::Left => "left".into(),
            Key::Right => "right".into(),
            Key::Up => "up".into(),
            Key::Down => "down".into(),
            Key::Home => "home".into(),
            Key::End => "end".into(),
            Key::PageUp => "pageup".into(),
            Key::PageDown => "pagedown".into(),
            Key::Letter(c) => char::from(c).to_string(),
            Key::Digit(c) => char::from(c).to_string(),
            Key::Function(n) => format!("f{n}"),
        }
    }
}

impl KeyCombo {
    pub fn canonical(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("ctrl".to_string());
        }
        if self.alt {
            parts.push("alt".to_string());
        }
        if self.shift {
            parts.push("shift".to_string());
        }
        if self.win {
            parts.push("win".to_string());
        }
        parts.push(self.key.name());
        parts.join("+")
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

/// Parse `ctrl+shift+enter`, `tab`, `cmd+c`. Modifiers are not a combo on
/// their own. Unknown tokens fail and nothing is injected.
pub fn parse_combo(raw: &str) -> Result<KeyCombo, String> {
    let text = raw.trim();
    if text.is_empty() || text.len() > 64 {
        return Err("key combo is empty or too long".into());
    }
    let mut combo = KeyCombo {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        key: Key::Enter,
    };
    let mut saw_key = false;
    for token in text.split('+') {
        let token = token.trim().to_ascii_lowercase();
        if token.is_empty() {
            return Err("key combo has an empty part".into());
        }
        match token.as_str() {
            "ctrl" | "control" => combo.ctrl = true,
            "alt" | "option" => combo.alt = true,
            "shift" => combo.shift = true,
            "win" | "super" | "meta" | "cmd" | "command" => combo.win = true,
            other => {
                if saw_key {
                    return Err(format!("key combo has two keys ({other})"));
                }
                combo.key = parse_key(other)?;
                saw_key = true;
            }
        }
    }
    if !saw_key {
        return Err("key combo needs a key, not only modifiers".into());
    }
    Ok(combo)
}

fn parse_key(token: &str) -> Result<Key, String> {
    Ok(match token {
        "enter" | "return" => Key::Enter,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "backspace" | "bksp" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "space" => Key::Space,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        other if other.len() == 1 => {
            let c = other.as_bytes()[0];
            if c.is_ascii_lowercase() {
                Key::Letter(c)
            } else if c.is_ascii_digit() {
                Key::Digit(c)
            } else {
                return Err(format!("unknown key {other}"));
            }
        }
        other if other.starts_with('f') && other.len() <= 3 => {
            let n: u8 = other[1..]
                .parse()
                .map_err(|_| format!("unknown key {other}"))?;
            if (1..=12).contains(&n) {
                Key::Function(n)
            } else {
                return Err(format!("unknown key {other}"));
            }
        }
        other => return Err(format!("unknown key {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_canonicalizes() {
        let combo = parse_combo("Ctrl+Shift+Enter").expect("parse");
        assert!(combo.ctrl && combo.shift && !combo.alt);
        assert_eq!(combo.key, Key::Enter);
        assert_eq!(combo.canonical(), "ctrl+shift+enter");
        assert_eq!(parse_combo("cmd+c").unwrap().canonical(), "win+c");
        assert_eq!(parse_combo("tab").unwrap().canonical(), "tab");
        assert_eq!(parse_combo("F5").unwrap().key, Key::Function(5));
    }

    #[test]
    fn rejects_junk_and_modifier_only() {
        assert!(parse_combo("").is_err());
        assert!(parse_combo("ctrl").is_err());
        assert!(parse_combo("ctrl+").is_err());
        assert!(parse_combo("ctrl+enter+tab").is_err());
        assert!(parse_combo("nope").is_err());
    }
}
