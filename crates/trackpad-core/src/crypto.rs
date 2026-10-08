//! Authenticated crypto: Noise handshakes, UDP session AEAD, replay window.
//!
//! Design (see `docs/protocol.md` §4):
//! - Pairing: `Noise_XXpsk3_25519_ChaChaPoly_Blake2s` over TCP. The QR
//!   secret (or the SPAKE2-derived key for short codes) is PSK slot 3.
//!   After the handshake the initiator checks the responder's static key
//!   against the QR fingerprint, binding the ceremony to the scanned code.
//! - Reconnect: `Noise_XX_25519_ChaChaPoly_Blake2s`, no PSK. Each side
//!   checks the peer static against its trust store. Trust-on-first-use
//!   happened during pairing; this only re-proves it.
//! - UDP transport does NOT use snow's `TransportState`: its nonce is a
//!   sequential counter, which desyncs on lossy UDP. Instead both sides
//!   HKDF-SHA256 the handshake hash into two directional 256-bit keys and
//!   seal each packet with ChaCha20-Poly1305, nonce = `0x00000000 || seq`.
//!   Fresh keys per handshake make seq-reuse across sessions impossible.
//! - AAD covers the full header, so version/session/seq/type cannot be
//!   altered without failing authentication.

use chacha20poly1305::aead::inout::InOutBuf;
use chacha20poly1305::{AeadInOut, ChaCha20Poly1305, Key, KeyInit, Nonce, Tag};
use hkdf::Hkdf;
use sha2::Sha256;
use snow::{Builder, HandshakeState};

/// Noise pattern for first-time pairing (mutual statics + PSK).
pub const PAIR_PATTERN: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
/// Noise pattern for reconnecting trusted devices (mutual statics, no PSK).
pub const RECONNECT_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
/// PSK slot used in [`PAIR_PATTERN`] (the `psk3` modifier).
pub const PAIR_PSK_INDEX: u8 = 3;
/// Noise static-key length (X25519).
pub const STATIC_KEY_LEN: usize = 32;
/// UDP session-key length (ChaCha20-Poly1305).
pub const SESSION_KEY_LEN: usize = 32;
/// Encrypted UDP envelope version byte (v1 = plain, Phase 2 and earlier).
pub const ENCRYPTED_VERSION: u8 = 2;
/// AEAD tag length appended to every encrypted packet.
pub const TAG_LEN: usize = 16;
/// Replay window width in packets.
pub const REPLAY_WINDOW: u64 = 1024;

/// Crypto failures. Secrets are never included in messages.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("noise handshake failed")]
    Noise(#[from] snow::Error),
    #[error("key derivation failed")]
    Hkdf(#[from] hkdf::InvalidLength),
    #[error("authentication failed")]
    Auth,
    #[error("bad encrypted packet: {0}")]
    Format(&'static str),
    #[error("unsupported envelope version: {0}")]
    BadVersion(u8),
    #[error("peer static key does not match")]
    UntrustedPeer,
}

/// A Noise handshake in progress. The caller drives the message sequence
/// (`write_message` on your turn, `read_message` on the peer's):
/// XX initiator: write, read, write. XX responder: read, write, read.
/// Payloads ride inside handshake messages 2 and 3 (message 1 carries none).
pub struct NoiseHandshake {
    state: HandshakeState,
}

impl NoiseHandshake {
    /// Initiator (phone) for pairing. `psk` is the QR secret or the
    /// SPAKE2-derived key.
    pub fn initiator_pair(local_priv: &[u8], psk: &[u8; 32]) -> Result<Self, CryptoError> {
        Self::build(PAIR_PATTERN, local_priv, Some(psk), true)
    }

    /// Responder (desktop) for pairing.
    pub fn responder_pair(local_priv: &[u8], psk: &[u8; 32]) -> Result<Self, CryptoError> {
        Self::build(PAIR_PATTERN, local_priv, Some(psk), false)
    }

    /// Initiator for reconnecting a trusted device (no PSK).
    pub fn initiator_reconnect(local_priv: &[u8]) -> Result<Self, CryptoError> {
        Self::build(RECONNECT_PATTERN, local_priv, None, true)
    }

    /// Responder for reconnecting a trusted device (no PSK).
    pub fn responder_reconnect(local_priv: &[u8]) -> Result<Self, CryptoError> {
        Self::build(RECONNECT_PATTERN, local_priv, None, false)
    }

    fn build(
        pattern: &str,
        local_priv: &[u8],
        psk: Option<&[u8; 32]>,
        initiator: bool,
    ) -> Result<Self, CryptoError> {
        let params = pattern
            .parse()
            .map_err(|_| CryptoError::Format("bad pattern"))?;
        let mut b = Builder::new(params);
        b = b.local_private_key(local_priv)?;
        if let Some(k) = psk {
            b = b.psk(PAIR_PSK_INDEX, k)?;
        }
        let state = if initiator {
            b.build_initiator()?
        } else {
            b.build_responder()?
        };
        Ok(Self { state })
    }

    /// True when all handshake messages are exchanged.
    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Emit this side's next handshake message with `payload` inside.
    pub fn write_message(&mut self, payload: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if self.state.is_handshake_finished() || !self.state.is_my_turn() {
            return Err(CryptoError::Format("not our turn to write"));
        }
        let mut out = vec![0u8; 65535];
        let n = self.state.write_message(payload, &mut out)?;
        Ok(out[..n].to_vec())
    }

    /// Consume the peer's next handshake message, returning its payload.
    pub fn read_message(&mut self, msg: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if self.state.is_handshake_finished() || self.state.is_my_turn() {
            return Err(CryptoError::Format("not our turn to read"));
        }
        let mut out = vec![0u8; 65535];
        let n = self.state.read_message(msg, &mut out)?;
        Ok(out[..n].to_vec())
    }

    /// The peer's static key, available once the handshake completes.
    pub fn remote_static(&self) -> Option<Vec<u8>> {
        self.state.get_remote_static().map(|s| s.to_vec())
    }

    /// Handshake hash for session-key derivation. Only valid when finished.
    pub fn handshake_hash(&self) -> Vec<u8> {
        self.state.get_handshake_hash().to_vec()
    }
}

/// Generate a fresh Noise static keypair. Returns `(private, public)`.
pub fn generate_keypair() -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
    let params = PAIR_PATTERN
        .parse()
        .map_err(|_| CryptoError::Format("bad pattern"))?;
    let kp = Builder::new(params).generate_keypair()?;
    Ok((kp.private, kp.public))
}

/// Directional UDP session keys derived from one handshake.
/// `Debug` is redacted: keys must never reach logs.
pub struct UdpKeys {
    /// Phone -> desktop (input packets, PING).
    pub phone_key: [u8; SESSION_KEY_LEN],
    /// Desktop -> phone (PONG, control acks).
    pub desktop_key: [u8; SESSION_KEY_LEN],
}

impl Clone for UdpKeys {
    fn clone(&self) -> Self {
        Self {
            phone_key: self.phone_key,
            desktop_key: self.desktop_key,
        }
    }
}

impl std::fmt::Debug for UdpKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpKeys")
            .field("phone_key", &"[redacted]")
            .field("desktop_key", &"[redacted]")
            .finish()
    }
}

/// HKDF-SHA256(handshake_hash) with domain-separated info strings.
pub fn derive_udp_keys(handshake_hash: &[u8]) -> Result<UdpKeys, CryptoError> {
    let hk = Hkdf::<Sha256>::new(None, handshake_hash);
    let mut phone_key = [0u8; SESSION_KEY_LEN];
    let mut desktop_key = [0u8; SESSION_KEY_LEN];
    hk.expand(b"wpt-udp-phone-v1", &mut phone_key)?;
    hk.expand(b"wpt-udp-desktop-v1", &mut desktop_key)?;
    Ok(UdpKeys {
        phone_key,
        desktop_key,
    })
}

/// Seal a packet into the v2 encrypted envelope:
/// `version:u8=2 | session:u64 | seq:u64 | type:u8 | ciphertext | tag:16`.
pub fn seal(
    key: &[u8; SESSION_KEY_LEN],
    session_id: u64,
    seq: u64,
    msg: crate::Message,
) -> Vec<u8> {
    let mut pkt = crate::encode(session_id, seq, msg);
    pkt[0] = ENCRYPTED_VERSION;
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    let nonce = Nonce::from(nonce_bytes(seq));
    let tag: Tag = {
        let (header, payload) = pkt.split_at_mut(crate::HEADER_LEN);
        cipher
            .encrypt_inout_detached(&nonce, header, InOutBuf::from(payload))
            .expect("seal cannot fail with valid key/nonce sizes")
    };
    pkt.extend_from_slice(tag.as_slice());
    pkt
}

/// Verify and open a v2 envelope. Returns `(session_id, seq, msg)`.
/// Replay checking is the caller's job ([`ReplayWindow`]).
pub fn open(
    key: &[u8; SESSION_KEY_LEN],
    data: &[u8],
) -> Result<(u64, u64, crate::Message), CryptoError> {
    if data.len() < crate::HEADER_LEN + TAG_LEN {
        return Err(CryptoError::Format("packet too short"));
    }
    if data.len() > crate::MAX_PACKET_LEN + TAG_LEN {
        return Err(CryptoError::Format("packet too long"));
    }
    if data[0] != ENCRYPTED_VERSION {
        return Err(CryptoError::BadVersion(data[0]));
    }
    let seq = u64::from_le_bytes(data[9..17].try_into().expect("sliced length checked"));
    let (ciphertext, tag_bytes) =
        data[crate::HEADER_LEN..].split_at(data.len() - crate::HEADER_LEN - TAG_LEN);
    let mut buf = ciphertext.to_vec();
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    let nonce = Nonce::from(nonce_bytes(seq));
    let tag = Tag::try_from(tag_bytes).map_err(|_| CryptoError::Auth)?;
    cipher
        .decrypt_inout_detached(
            &nonce,
            &data[..crate::HEADER_LEN],
            InOutBuf::from(buf.as_mut_slice()),
            &tag,
        )
        .map_err(|_| CryptoError::Auth)?;
    // Rebuild a v1 image for the existing strict decoder.
    let mut v1 = Vec::with_capacity(crate::HEADER_LEN + buf.len());
    v1.push(crate::PROTOCOL_VERSION);
    v1.extend_from_slice(&data[1..crate::HEADER_LEN]);
    v1.extend_from_slice(&buf);
    let pkt = crate::decode(&v1).map_err(|_| CryptoError::Auth)?;
    Ok((pkt.session_id, pkt.seq, pkt.msg))
}

/// 96-bit nonce material: 4 zero bytes + 64-bit sequence, little-endian.
fn nonce_bytes(seq: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&seq.to_le_bytes());
    n
}

/// Sliding replay window over the newest [`REPLAY_WINDOW`] sequences.
/// Bit `i` (0 = oldest) covers `highest - 1023 + i`. Not thread-safe; one
/// per session, reset on every new handshake (fresh keys anyway).
#[derive(Debug)]
pub struct ReplayWindow {
    highest: Option<u64>,
    bits: [u64; 16],
}

impl ReplayWindow {
    pub fn new() -> Self {
        Self {
            highest: None,
            bits: [0; 16],
        }
    }

    /// Returns true if `seq` is fresh (records it); false for duplicates
    /// and packets older than the window.
    pub fn check(&mut self, seq: u64) -> bool {
        match self.highest {
            None => {
                self.highest = Some(seq);
                self.bits = [0; 16];
                self.bits[15] = 1u64 << 63;
                true
            }
            Some(h) if seq > h => {
                shift_right(&mut self.bits, seq - h);
                self.highest = Some(seq);
                self.bits[15] |= 1u64 << 63;
                true
            }
            Some(h) => {
                let age = h - seq;
                if age >= REPLAY_WINDOW {
                    return false;
                }
                let idx = (REPLAY_WINDOW - 1 - age) as usize;
                let w = idx / 64;
                let b = idx % 64;
                if self.bits[w] & (1u64 << b) != 0 {
                    return false;
                }
                self.bits[w] |= 1u64 << b;
                true
            }
        }
    }
}

/// Shift the 1024-bit array right by `shift` (drops the oldest bits).
fn shift_right(bits: &mut [u64; 16], shift: u64) {
    if shift >= REPLAY_WINDOW {
        *bits = [0; 16];
        return;
    }
    let word = (shift / 64) as usize;
    let bit = (shift % 64) as usize;
    let mut next = [0u64; 16];
    for (i, slot) in next.iter_mut().enumerate() {
        let src = i + word;
        if src >= 16 {
            continue;
        }
        let mut v = bits[src] >> bit;
        if bit > 0 && src + 1 < 16 {
            v |= bits[src + 1] << (64 - bit);
        }
        *slot = v;
    }
    *bits = next;
}

impl Default for ReplayWindow {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair_handshake() -> (Vec<u8>, UdpKeys, Vec<u8>, UdpKeys) {
        let (dp, _) = generate_keypair().expect("keygen");
        let (pp, _) = generate_keypair().expect("keygen");
        let psk = [7u8; 32];
        let mut init = NoiseHandshake::initiator_pair(&pp, &psk).expect("initiator");
        let mut resp = NoiseHandshake::responder_pair(&dp, &psk).expect("responder");
        // XX: -> e; <- e, ee, s, es; -> s, se.
        let m1 = init.write_message(&[]).expect("m1");
        let _ = resp.read_message(&m1).expect("m1 payload");
        let m2 = resp.write_message(&[]).expect("m2");
        let _ = init.read_message(&m2).expect("m2 payload");
        let m3 = init.write_message(b"phone-name").expect("m3");
        let payload = resp.read_message(&m3).expect("m3 payload");
        assert!(init.is_finished() && resp.is_finished());
        assert_eq!(payload, b"phone-name");
        let init_keys = derive_udp_keys(&init.handshake_hash()).expect("keys");
        let resp_keys = derive_udp_keys(&resp.handshake_hash()).expect("keys");
        assert_eq!(init_keys.phone_key, resp_keys.phone_key);
        assert_eq!(init_keys.desktop_key, resp_keys.desktop_key);
        let init_static = init.remote_static().expect("init sees responder static");
        let resp_static = resp.remote_static().expect("resp sees initiator static");
        (init_static, init_keys, resp_static, resp_keys)
    }

    #[test]
    fn pair_handshake_agrees_on_keys_and_statics() {
        let (init_view, _, resp_view, _) = pair_handshake();
        assert_eq!(init_view.len(), STATIC_KEY_LEN);
        assert_eq!(resp_view.len(), STATIC_KEY_LEN);
    }

    #[test]
    fn wrong_psk_fails() {
        let (dp, _) = generate_keypair().expect("keygen");
        let (pp, _) = generate_keypair().expect("keygen");
        let mut init = NoiseHandshake::initiator_pair(&pp, &[1u8; 32]).expect("initiator");
        let mut resp = NoiseHandshake::responder_pair(&dp, &[2u8; 32]).expect("responder");
        // PSK first mixes into message 3, so m1/m2 still decode.
        let m1 = init.write_message(&[]).expect("m1");
        let _ = resp.read_message(&m1).expect("m1 reads (e is plain)");
        let m2 = resp.write_message(&[]).expect("m2");
        let _ = init.read_message(&m2).expect("m2 has no PSK yet");
        let m3 = init.write_message(&[]).expect("m3 writes (wrong PSK)");
        // Responder authenticates m3 under its own PSK: must fail here.
        assert!(
            resp.read_message(&m3).is_err(),
            "PSK mismatch must fail at m3"
        );
        assert!(!resp.is_finished());
    }

    #[test]
    fn seal_open_round_trip() {
        let (_, keys, _, _) = pair_handshake();
        let pkt = seal(
            &keys.phone_key,
            11,
            42,
            crate::Message::Move { dx: 5, dy: -3 },
        );
        assert_eq!(pkt[0], ENCRYPTED_VERSION);
        let (sess, seq, msg) = open(&keys.phone_key, &pkt).expect("open");
        assert_eq!(
            (sess, seq, msg),
            (11, 42, crate::Message::Move { dx: 5, dy: -3 })
        );
    }

    #[test]
    fn tampered_ciphertext_and_header_rejected() {
        let (_, keys, _, _) = pair_handshake();
        let mut pkt = seal(
            &keys.phone_key,
            11,
            42,
            crate::Message::Ping { timestamp_ms: 1 },
        );
        pkt[20] ^= 0xFF;
        assert!(matches!(
            open(&keys.phone_key, &pkt),
            Err(CryptoError::Auth)
        ));
        let mut pkt = seal(
            &keys.phone_key,
            11,
            42,
            crate::Message::Ping { timestamp_ms: 1 },
        );
        pkt[10] ^= 0xFF; // seq byte in AAD
        assert!(matches!(
            open(&keys.phone_key, &pkt),
            Err(CryptoError::Auth)
        ));
        let wrong = [9u8; 32];
        let pkt = seal(
            &keys.phone_key,
            11,
            42,
            crate::Message::Ping { timestamp_ms: 1 },
        );
        assert!(matches!(open(&wrong, &pkt), Err(CryptoError::Auth)));
    }

    #[test]
    fn replay_window_accepts_once_and_reorders() {
        let mut w = ReplayWindow::new();
        assert!(w.check(100));
        assert!(!w.check(100));
        assert!(w.check(101));
        assert!(w.check(99)); // out of order inside window
        assert!(!w.check(99));
        assert!(w.check(0)); // age 101 < window: accepted, then spent
        assert!(!w.check(0));
    }

    #[test]
    fn replay_window_rejects_stale_and_handles_jumps() {
        let mut w = ReplayWindow::new();
        assert!(w.check(5000));
        assert!(!w.check(5000 - REPLAY_WINDOW)); // exactly window-old
        assert!(w.check(5000 - REPLAY_WINDOW + 1)); // edge still inside
        assert!(w.check(1_000_000)); // big jump clears window
        assert!(!w.check(5000)); // pre-jump seq is now ancient
        assert!(w.check(1_000_001));
    }
}
