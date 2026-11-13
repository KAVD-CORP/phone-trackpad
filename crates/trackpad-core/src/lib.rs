//! `trackpad-core`: shared protocol between phone and desktop.
//!
//! Phase 1: versioned binary codec for MOVE / CLICK / PING / PONG over
//! plain UDP. No encryption, no replay window yet (Phase 4). The exact
//! same bytes are produced/consumed by the Rust desktop service and the
//! Flutter sender (`apps/mobile/lib/net.dart` mirrors this layout).
//!
//! Wire layout (all integers little-endian):
//!
//! ```text
//! version:u8 | session_id:u64 | seq:u64 | msg_type:u8 | payload
//! ```
//!
//! Payloads: MOVE `dx:i16 | dy:i16`, CLICK `button:u8` (0 = left),
//! PING / PONG `timestamp_ms:u64` (echoed back for RTT measurement),
//! TEXT raw UTF-8 (ADR-0005), KEY `usage:u16 | modifiers:u8 | state:u8`.

pub mod crypto;
pub mod pairing;

/// Protocol version. Reject anything else.
pub const PROTOCOL_VERSION: u8 = 1;

/// Default UDP port (hard-coded in Phase 1; discovery lands in Phase 4).
pub const DEFAULT_UDP_PORT: u16 = 51515;

/// Default TCP port for pairing and reconnect handshakes (Phase 4).
pub const DEFAULT_TCP_PORT: u16 = 51516;

/// Largest accepted datagram. TEXT carries up to 512 UTF-8 bytes over the
/// 18-byte header (`MAX_TEXT_LEN`); anything larger is rejected without
/// further parsing.
pub const MAX_PACKET_LEN: usize = 1024;

/// Maximum UTF-8 payload of one TEXT message (ADR-0005). Senders must
/// split longer runs; this keeps a TEXT datagram inside `MAX_PACKET_LEN`.
pub const MAX_TEXT_LEN: usize = 512;

/// Header length: version(1) + session_id(8) + seq(8) + msg_type(1).
pub const HEADER_LEN: usize = 18;

/// Left mouse button id for CLICK.
pub const BUTTON_LEFT: u8 = 0;

pub mod msg_type {
    /// One-finger relative movement, payload `dx:i16 | dy:i16`.
    pub const MOVE: u8 = 0x01;
    /// Simple click, payload `button:u8`.
    pub const CLICK: u8 = 0x02;
    /// Latency probe, payload `timestamp_ms:u64`.
    pub const PING: u8 = 0x03;
    /// Probe reply echoing the PING timestamp, payload `timestamp_ms:u64`.
    pub const PONG: u8 = 0x04;
    /// Press-and-hold a button, payload `button:u8`. Critical: the desktop
    /// releases held buttons on session change, silence timeout, or revoke.
    pub const BUTTON_DOWN: u8 = 0x05;
    /// Release a button, payload `button:u8`. Sent redundantly by senders.
    pub const BUTTON_UP: u8 = 0x06;
    /// Two-finger scroll, payload `dx:i16 | dy:i16` in wheel ticks scaled
    /// by the sender (positive dy = scroll down in natural mode).
    pub const SCROLL: u8 = 0x07;
    /// Typed characters, payload raw UTF-8 (1..=[`MAX_TEXT_LEN`] bytes).
    /// Injected as Unicode input; no key state, so nothing to guard.
    pub const TEXT: u8 = 0x08;
    /// One key event, payload `usage:u16 | modifiers:u8 | state:u8`.
    /// `usage` is a USB HID keyboard usage; the desktop maps it to the
    /// platform code. Held keys are guarded like buttons.
    pub const KEY: u8 = 0x09;
}

/// Keyboard-event constants (ADR-0005).
pub mod key {
    /// Modifier bitmask: bit 0 Shift, 1 Ctrl, 2 Alt, 3 Meta.
    pub const MOD_SHIFT: u8 = 1 << 0;
    pub const MOD_CTRL: u8 = 1 << 1;
    pub const MOD_ALT: u8 = 1 << 2;
    pub const MOD_META: u8 = 1 << 3;
    /// KEY state byte.
    pub const STATE_UP: u8 = 0;
    pub const STATE_DOWN: u8 = 1;

    /// HID usages the tests and the non-text key row rely on.
    pub mod usage {
        pub const ENTER: u16 = 0x28;
        pub const ESCAPE: u16 = 0x29;
        pub const BACKSPACE: u16 = 0x2a;
        pub const TAB: u16 = 0x2b;
        pub const LEFT: u16 = 0x50;
        pub const RIGHT: u16 = 0x4f;
        pub const UP: u16 = 0x52;
        pub const DOWN: u16 = 0x51;
        pub const DELETE: u16 = 0x4c;
    }
}

/// Mouse button ids shared by CLICK / BUTTON_DOWN / BUTTON_UP.
pub mod button {
    pub const LEFT: u8 = 0;
    pub const RIGHT: u8 = 1;
    pub const MIDDLE: u8 = 2;
}

/// A decoded input/latency message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Move { dx: i16, dy: i16 },
    Click { button: u8 },
    Ping { timestamp_ms: u64 },
    Pong { timestamp_ms: u64 },
    ButtonDown { button: u8 },
    ButtonUp { button: u8 },
    Scroll { dx: i16, dy: i16 },
    /// Typed characters (ADR-0005). Max [`MAX_TEXT_LEN`] UTF-8 bytes.
    Text { text: String },
    /// One key event: HID usage + modifier bitmask + down/up state.
    Key { usage: u16, modifiers: u8, state: u8 },
}

/// A decoded packet: routing envelope + message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub session_id: u64,
    pub seq: u64,
    pub msg: Message,
}

/// Decode failures. Never panics; malformed input maps to a variant.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("packet too short: {0} bytes")]
    TooShort(usize),
    #[error("packet too long: {0} bytes")]
    TooLong(usize),
    #[error("unsupported version: {0}")]
    BadVersion(u8),
    #[error("unknown message type: {0:#04x}")]
    UnknownType(u8),
    #[error("trailing bytes: {0} extra")]
    Trailing(usize),
    #[error("text is not valid utf-8")]
    BadText,
}

/// Serialize a packet. Returns a fresh buffer (buffer reuse is Phase 5).
pub fn encode(session_id: u64, seq: u64, msg: Message) -> Vec<u8> {
    let mut out = Vec::with_capacity(26);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&session_id.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    match msg {
        Message::Move { dx, dy } => {
            out.push(msg_type::MOVE);
            out.extend_from_slice(&dx.to_le_bytes());
            out.extend_from_slice(&dy.to_le_bytes());
        }
        Message::Click { button } => {
            out.push(msg_type::CLICK);
            out.push(button);
        }
        Message::Ping { timestamp_ms } => {
            out.push(msg_type::PING);
            out.extend_from_slice(&timestamp_ms.to_le_bytes());
        }
        Message::Pong { timestamp_ms } => {
            out.push(msg_type::PONG);
            out.extend_from_slice(&timestamp_ms.to_le_bytes());
        }
        Message::ButtonDown { button } => {
            out.push(msg_type::BUTTON_DOWN);
            out.push(button);
        }
        Message::ButtonUp { button } => {
            out.push(msg_type::BUTTON_UP);
            out.push(button);
        }
        Message::Scroll { dx, dy } => {
            out.push(msg_type::SCROLL);
            out.extend_from_slice(&dx.to_le_bytes());
            out.extend_from_slice(&dy.to_le_bytes());
        }
        Message::Text { text } => {
            out.push(msg_type::TEXT);
            out.extend_from_slice(text.as_bytes());
        }
        Message::Key {
            usage,
            modifiers,
            state,
        } => {
            out.push(msg_type::KEY);
            out.extend_from_slice(&usage.to_le_bytes());
            out.push(modifiers);
            out.push(state);
        }
    }
    out
}

/// Deserialize and strictly validate one datagram.
pub fn decode(data: &[u8]) -> Result<Packet, DecodeError> {
    if data.len() > MAX_PACKET_LEN {
        return Err(DecodeError::TooLong(data.len()));
    }
    if data.len() < HEADER_LEN {
        return Err(DecodeError::TooShort(data.len()));
    }
    if data[0] != PROTOCOL_VERSION {
        return Err(DecodeError::BadVersion(data[0]));
    }
    let session_id = u64::from_le_bytes(data[1..9].try_into().expect("sliced length checked"));
    let seq = u64::from_le_bytes(data[9..17].try_into().expect("sliced length checked"));
    let body = &data[HEADER_LEN..];
    let msg = match data[17] {
        msg_type::MOVE => {
            if body.len() != 4 {
                return Err(if body.len() < 4 {
                    DecodeError::TooShort(data.len())
                } else {
                    DecodeError::Trailing(body.len() - 4)
                });
            }
            Message::Move {
                dx: i16::from_le_bytes([body[0], body[1]]),
                dy: i16::from_le_bytes([body[2], body[3]]),
            }
        }
        msg_type::CLICK => {
            if body.len() != 1 {
                return Err(if body.is_empty() {
                    DecodeError::TooShort(data.len())
                } else {
                    DecodeError::Trailing(body.len() - 1)
                });
            }
            Message::Click { button: body[0] }
        }
        msg_type::PING => {
            if body.len() != 8 {
                return Err(if body.len() < 8 {
                    DecodeError::TooShort(data.len())
                } else {
                    DecodeError::Trailing(body.len() - 8)
                });
            }
            Message::Ping {
                timestamp_ms: u64::from_le_bytes(body.try_into().expect("length checked")),
            }
        }
        msg_type::PONG => {
            if body.len() != 8 {
                return Err(if body.len() < 8 {
                    DecodeError::TooShort(data.len())
                } else {
                    DecodeError::Trailing(body.len() - 8)
                });
            }
            Message::Pong {
                timestamp_ms: u64::from_le_bytes(body.try_into().expect("length checked")),
            }
        }
        msg_type::BUTTON_DOWN => {
            if body.len() != 1 {
                return Err(if body.is_empty() {
                    DecodeError::TooShort(data.len())
                } else {
                    DecodeError::Trailing(body.len() - 1)
                });
            }
            Message::ButtonDown { button: body[0] }
        }
        msg_type::BUTTON_UP => {
            if body.len() != 1 {
                return Err(if body.is_empty() {
                    DecodeError::TooShort(data.len())
                } else {
                    DecodeError::Trailing(body.len() - 1)
                });
            }
            Message::ButtonUp { button: body[0] }
        }
        msg_type::SCROLL => {
            if body.len() != 4 {
                return Err(if body.len() < 4 {
                    DecodeError::TooShort(data.len())
                } else {
                    DecodeError::Trailing(body.len() - 4)
                });
            }
            Message::Scroll {
                dx: i16::from_le_bytes([body[0], body[1]]),
                dy: i16::from_le_bytes([body[2], body[3]]),
            }
        }
        msg_type::TEXT => {
            if body.is_empty() || body.len() > MAX_TEXT_LEN {
                return Err(if body.is_empty() {
                    DecodeError::TooShort(data.len())
                } else {
                    DecodeError::TooLong(data.len())
                });
            }
            let text = String::from_utf8(body.to_vec()).map_err(|_| DecodeError::BadText)?;
            Message::Text { text }
        }
        msg_type::KEY => {
            if body.len() != 4 {
                return Err(if body.len() < 4 {
                    DecodeError::TooShort(data.len())
                } else {
                    DecodeError::Trailing(body.len() - 4)
                });
            }
            Message::Key {
                usage: u16::from_le_bytes([body[0], body[1]]),
                modifiers: body[2],
                state: body[3],
            }
        }
        t => return Err(DecodeError::UnknownType(t)),
    };
    Ok(Packet {
        session_id,
        seq,
        msg,
    })
}

/// Backwards-compatible hello used by the Phase 0 smoke test.
pub fn hello() -> &'static str {
    "trackpad-core hello"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_mentions_core() {
        assert!(hello().contains("trackpad-core"));
    }

    #[test]
    fn move_round_trip() {
        let p = Packet {
            session_id: 0x0102_0304_0506_0708,
            seq: 42,
            msg: Message::Move { dx: -300, dy: 1024 },
        };
        let bytes = encode(p.session_id, p.seq, p.msg.clone());
        assert_eq!(bytes.len(), HEADER_LEN + 4);
        assert_eq!(decode(&bytes), Ok(p));
    }

    #[test]
    fn click_ping_pong_round_trip() {
        for msg in [
            Message::Click {
                button: BUTTON_LEFT,
            },
            Message::Ping { timestamp_ms: 1 },
            Message::Pong {
                timestamp_ms: u64::MAX,
            },
            Message::ButtonDown {
                button: button::RIGHT,
            },
            Message::ButtonUp {
                button: button::MIDDLE,
            },
            Message::Scroll { dx: -120, dy: 240 },
        ] {
            let bytes = encode(7, 9, msg.clone());
            assert_eq!(
                decode(&bytes),
                Ok(Packet {
                    session_id: 7,
                    seq: 9,
                    msg
                })
            );
        }
    }

    #[test]
    fn rejects_bad_version() {
        let mut bytes = encode(1, 1, Message::Ping { timestamp_ms: 0 });
        bytes[0] = 0xFF;
        assert_eq!(decode(&bytes), Err(DecodeError::BadVersion(0xFF)));
    }

    #[test]
    fn rejects_truncated_and_empty() {
        assert_eq!(decode(&[]), Err(DecodeError::TooShort(0)));
        let bytes = encode(1, 1, Message::Move { dx: 1, dy: 2 });
        for len in [1, HEADER_LEN - 1, HEADER_LEN + 1] {
            assert!(matches!(
                decode(&bytes[..len]),
                Err(DecodeError::TooShort(_))
            ));
        }
    }

    #[test]
    fn rejects_oversized_and_trailing() {
        assert!(matches!(
            decode(&[0u8; MAX_PACKET_LEN + 1]),
            Err(DecodeError::TooLong(_))
        ));
        let mut bytes = encode(1, 1, Message::Click { button: 0 });
        bytes.push(0xFF);
        assert_eq!(decode(&bytes), Err(DecodeError::Trailing(1)));
        let mut unknown = [0u8; HEADER_LEN];
        unknown[0] = PROTOCOL_VERSION;
        unknown[17] = 0x7F;
        assert_eq!(decode(&unknown), Err(DecodeError::UnknownType(0x7F)));
    }

    #[test]
    fn text_and_key_round_trip() {
        let msgs = [
            Message::Text {
                text: "héllo → 世界".to_string(),
            },
            Message::Key {
                usage: key::usage::ENTER,
                modifiers: key::MOD_CTRL,
                state: key::STATE_DOWN,
            },
            Message::Key {
                usage: key::usage::LEFT,
                modifiers: 0,
                state: key::STATE_UP,
            },
        ];
        for msg in msgs {
            let bytes = encode(7, 9, msg.clone());
            assert_eq!(
                decode(&bytes),
                Ok(Packet {
                    session_id: 7,
                    seq: 9,
                    msg
                })
            );
        }
        // Key payload is exactly 4 bytes: usage u16 + modifiers u8 + state u8.
        let bytes = encode(7, 9, Message::Key { usage: 0x28, modifiers: 0, state: 1 });
        assert_eq!(bytes.len(), HEADER_LEN + 4);
    }

    #[test]
    fn text_enforces_max_length_and_utf8() {
        let long = "x".repeat(MAX_TEXT_LEN + 1);
        let bytes = encode(1, 1, Message::Text { text: long });
        assert!(matches!(decode(&bytes), Err(DecodeError::TooLong(_))));
        let mut bytes = encode(1, 1, Message::Text { text: "ok".to_string() });
        bytes[19] = 0xFF; // first payload byte, invalid UTF-8 start
        assert_eq!(decode(&bytes), Err(DecodeError::BadText));
        let empty = encode(1, 1, Message::Text { text: String::new() });
        assert!(matches!(decode(&empty), Err(DecodeError::TooShort(_))));
    }

    #[test]
    fn rejects_unknown_type() {
        let mut bytes = encode(1, 1, Message::Ping { timestamp_ms: 0 });
        bytes[17] = 0x7F;
        assert_eq!(decode(&bytes), Err(DecodeError::UnknownType(0x7F)));
    }
}
