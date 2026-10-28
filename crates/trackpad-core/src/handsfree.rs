//! Handsfree control frames. These are not motion packets.
//!
//! A spoken command, a typed string, or a key combo is reliable, so it
//! travels on TCP (same rule as a click). The body is authenticated with a
//! key derived from the phone's UDP session key, and the nonce domain does
//! not overlap UDP, so a Handsfree sequence cannot reuse a motion nonce.
//!
//! Cleartext layout, little-endian, before encryption:
//!
//! ```text
//! 'H' 'F' | version:u8=1 | kind:u8 | session:u64 | seq:u64 | payload
//! ```
//!
//! The sealed body keeps that 20-byte header as AAD and appends the
//! ChaCha20-Poly1305 ciphertext and 16-byte tag. Session 0 is rejected.

use chacha20poly1305::aead::inout::InOutBuf;
use chacha20poly1305::{AeadInOut, ChaCha20Poly1305, Key, KeyInit, Nonce, Tag};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::crypto::SESSION_KEY_LEN;

/// Handsfree frame version. Independent of the motion protocol version.
pub const VERSION: u8 = 1;
/// `HF` + version + kind + session + seq.
pub const HEADER_LEN: usize = 20;
pub const TAG_LEN: usize = 16;
/// Spoken audio cap (a short command, not a meeting).
pub const MAX_VOICE_LEN: usize = 1_048_576;
/// Typed command, keyboard text, and key-combo strings.
pub const MAX_TEXT_LEN: usize = 8_192;

pub mod kind {
    pub const VOICE: u8 = 1;
    pub const COMMAND: u8 = 2;
    pub const TYPE_TEXT: u8 = 3;
    pub const KEY_COMBO: u8 = 4;
    pub const CANCEL: u8 = 5;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plain {
    pub session_id: u64,
    pub seq: u64,
    pub kind: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandsfreeError {
    #[error("session 0 is invalid")]
    SessionZero,
    #[error("sequence 0 is invalid")]
    SeqZero,
    #[error("unknown handsfree kind: {0}")]
    BadKind(u8),
    #[error("payload is too large")]
    TooLarge,
    #[error("handsfree frame is truncated")]
    Truncated,
    #[error("bad handsfree magic")]
    BadMagic,
    #[error("unsupported handsfree version: {0}")]
    BadVersion(u8),
    #[error("handsfree authentication failed")]
    Auth,
}

fn payload_limit(kind: u8) -> Result<usize, HandsfreeError> {
    match kind {
        kind::VOICE => Ok(MAX_VOICE_LEN),
        kind::COMMAND | kind::TYPE_TEXT | kind::KEY_COMBO => Ok(MAX_TEXT_LEN),
        kind::CANCEL => Ok(0),
        other => Err(HandsfreeError::BadKind(other)),
    }
}

/// Encode the cleartext frame. The TypeScript oracle in `src/protocol/handsfree.ts`
/// must produce the same bytes.
pub fn encode_plain(frame: &Plain) -> Result<Vec<u8>, HandsfreeError> {
    if frame.session_id == 0 {
        return Err(HandsfreeError::SessionZero);
    }
    if frame.seq == 0 {
        return Err(HandsfreeError::SeqZero);
    }
    let limit = payload_limit(frame.kind)?;
    if frame.payload.len() > limit {
        return Err(HandsfreeError::TooLarge);
    }
    let mut out = Vec::with_capacity(HEADER_LEN + frame.payload.len());
    out.push(b'H');
    out.push(b'F');
    out.push(VERSION);
    out.push(frame.kind);
    out.extend_from_slice(&frame.session_id.to_le_bytes());
    out.extend_from_slice(&frame.seq.to_le_bytes());
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

pub fn decode_plain(bytes: &[u8]) -> Result<Plain, HandsfreeError> {
    if bytes.len() < HEADER_LEN {
        return Err(HandsfreeError::Truncated);
    }
    if bytes[0] != b'H' || bytes[1] != b'F' {
        return Err(HandsfreeError::BadMagic);
    }
    if bytes[2] != VERSION {
        return Err(HandsfreeError::BadVersion(bytes[2]));
    }
    let kind = bytes[3];
    let session_id = u64::from_le_bytes(bytes[4..12].try_into().expect("header length"));
    let seq = u64::from_le_bytes(bytes[12..20].try_into().expect("header length"));
    if session_id == 0 {
        return Err(HandsfreeError::SessionZero);
    }
    if seq == 0 {
        return Err(HandsfreeError::SeqZero);
    }
    let payload = bytes[HEADER_LEN..].to_vec();
    let limit = payload_limit(kind)?;
    if payload.len() > limit {
        return Err(HandsfreeError::TooLarge);
    }
    Ok(Plain {
        session_id,
        seq,
        kind,
        payload,
    })
}

/// Domain-separated key so a Handsfree nonce cannot collide with UDP.
pub fn derive_key(phone_key: &[u8; SESSION_KEY_LEN]) -> [u8; SESSION_KEY_LEN] {
    let hk = Hkdf::<Sha256>::new(None, phone_key);
    let mut out = [0u8; SESSION_KEY_LEN];
    hk.expand(b"wpt-handsfree-phone-v1", &mut out)
        .expect("hkdf expand cannot fail for 32 bytes");
    out
}

/// Nonce is `0x01 || seq`, not UDP's `0x00 || seq`.
fn nonce_bytes(seq: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[0] = 0x01;
    n[4..].copy_from_slice(&seq.to_le_bytes());
    n
}

/// Seal a Handsfree body. Header stays in the clear and is the AAD.
pub fn seal(
    phone_key: &[u8; SESSION_KEY_LEN],
    session_id: u64,
    seq: u64,
    kind: u8,
    payload: &[u8],
) -> Result<Vec<u8>, HandsfreeError> {
    let mut body = encode_plain(&Plain {
        session_id,
        seq,
        kind,
        payload: payload.to_vec(),
    })?;
    let key = derive_key(phone_key);
    let cipher = ChaCha20Poly1305::new(&Key::from(key));
    let nonce = Nonce::from(nonce_bytes(seq));
    let tag: Tag = {
        let (header, plaintext) = body.split_at_mut(HEADER_LEN);
        cipher
            .encrypt_inout_detached(&nonce, header, InOutBuf::from(plaintext))
            .map_err(|_| HandsfreeError::Auth)?
    };
    body.extend_from_slice(tag.as_slice());
    Ok(body)
}

pub fn open(phone_key: &[u8; SESSION_KEY_LEN], data: &[u8]) -> Result<Plain, HandsfreeError> {
    if data.len() < HEADER_LEN + TAG_LEN {
        return Err(HandsfreeError::Truncated);
    }
    let (header_and_cipher, tag_bytes) = data.split_at(data.len() - TAG_LEN);
    if header_and_cipher.len() < HEADER_LEN {
        return Err(HandsfreeError::Truncated);
    }
    let seq = u64::from_le_bytes(header_and_cipher[12..20].try_into().expect("header length"));
    let key = derive_key(phone_key);
    let cipher = ChaCha20Poly1305::new(&Key::from(key));
    let nonce = Nonce::from(nonce_bytes(seq));
    let tag = Tag::try_from(tag_bytes).map_err(|_| HandsfreeError::Auth)?;
    let mut body = header_and_cipher.to_vec();
    {
        let (header, ciphertext) = body.split_at_mut(HEADER_LEN);
        cipher
            .decrypt_inout_detached(&nonce, header, InOutBuf::from(ciphertext), &tag)
            .map_err(|_| HandsfreeError::Auth)?;
    }
    decode_plain(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Plain {
        Plain {
            session_id: 0x0102_0304_0506_0708,
            seq: 7,
            kind: kind::COMMAND,
            payload: b"hi".to_vec(),
        }
    }

    #[test]
    fn golden_cleartext_matches_the_typescript_oracle() {
        let bytes = encode_plain(&sample()).expect("encode");
        assert_eq!(
            bytes,
            [
                0x48, 0x46, 0x01, 0x02, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0x07, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, b'h', b'i',
            ]
        );
        assert_eq!(decode_plain(&bytes).unwrap(), sample());
    }

    #[test]
    fn rejects_session_zero_and_oversize_and_bad_kind() {
        let mut frame = sample();
        frame.session_id = 0;
        assert_eq!(encode_plain(&frame), Err(HandsfreeError::SessionZero));
        frame.session_id = 1;
        frame.seq = 0;
        assert_eq!(encode_plain(&frame), Err(HandsfreeError::SeqZero));
        frame.seq = 1;
        frame.kind = 9;
        assert!(matches!(
            encode_plain(&frame),
            Err(HandsfreeError::BadKind(9))
        ));
        frame.kind = kind::CANCEL;
        frame.payload = b"x".to_vec();
        assert_eq!(encode_plain(&frame), Err(HandsfreeError::TooLarge));
    }

    #[test]
    fn seal_open_round_trip_and_tamper_and_udp_key_mismatch() {
        let phone_key = [7u8; 32];
        let sealed = seal(&phone_key, 4, 3, kind::TYPE_TEXT, b"Hello").expect("seal");
        assert_eq!(&sealed[0..2], b"HF");
        let opened = open(&phone_key, &sealed).expect("open");
        assert_eq!(opened.payload, b"Hello");
        assert_eq!(opened.kind, kind::TYPE_TEXT);
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xff;
        assert_eq!(open(&phone_key, &tampered), Err(HandsfreeError::Auth));
        let mut other = [7u8; 32];
        other[0] = 8;
        assert_eq!(open(&other, &sealed), Err(HandsfreeError::Auth));
        assert!(crate::decode(&sealed).is_err());
    }

    #[test]
    fn cancel_seals_an_empty_payload() {
        let phone_key = [1u8; 32];
        let sealed = seal(&phone_key, 2, 1, kind::CANCEL, b"").expect("seal");
        let opened = open(&phone_key, &sealed).expect("open");
        assert_eq!(opened.kind, kind::CANCEL);
        assert!(opened.payload.is_empty());
    }
}
