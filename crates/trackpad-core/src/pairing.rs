//! Pairing ceremony: QR secret lifecycle, short-code PAKE, QR payload codec,
//! attempt rate limiting, and trust-store record types.
//!
//! Two ceremonies share the Noise machinery (`crypto::PAIR_PATTERN`):
//! - QR: desktop shows a random 128-bit secret (TTL 2 min, single use).
//!   The secret travels inside the scanned code, so it is a PSK directly.
//! - Short code: desktop shows 6 digits; both sides run SPAKE2 over TCP and
//!   use the 32-byte output as the PSK. Offline brute force is impossible:
//!   each guess needs a live, rate-limited server round.
//!
//! All time is caller milliseconds (`now_ms`) so tests use fake clocks.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use spake2::{Ed25519Group, Identity, Password, Spake2};

/// Pairing-secret length in bytes (128 bit).
pub const PAIRING_SECRET_LEN: usize = 16;
/// Pairing ceremony TTL in milliseconds (2 min).
pub const PAIRING_TTL_MS: u64 = 120_000;
/// Max failed pairing attempts before lockout.
pub const MAX_ATTEMPTS: u32 = 3;
/// Lockout duration after [`MAX_ATTEMPTS`] failures.
pub const LOCKOUT_MS: u64 = 30_000;
/// QR envelope magic (ASCII) + format version.
pub const QR_MAGIC: &[u8; 4] = b"WPT1";
pub const QR_VERSION: u8 = 1;
/// Short-code length in digits.
pub const SHORT_CODE_DIGITS: u32 = 6;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("pairing secret expired")]
    Expired,
    #[error("pairing secret already used")]
    Reused,
    #[error("too many attempts, locked out")]
    LockedOut,
    #[error("malformed QR payload")]
    BadQr,
    #[error("short code exchange failed")]
    Spake2,
    #[error("device name too long")]
    NameTooLong,
}

/// A single-use, expiring pairing secret shown as QR.
#[derive(Debug, Clone)]
pub struct PairingSecret {
    bytes: [u8; PAIRING_SECRET_LEN],
    expires_at_ms: u64,
    used: bool,
}

impl PairingSecret {
    /// Generate from OS randomness. Fails only if the platform RNG fails.
    pub fn generate(now_ms: u64) -> Result<Self, PairingError> {
        let mut bytes = [0u8; PAIRING_SECRET_LEN];
        getrandom::fill(&mut bytes).map_err(|_| PairingError::Expired)?;
        Ok(Self::from_bytes(bytes, now_ms))
    }

    pub fn from_bytes(bytes: [u8; PAIRING_SECRET_LEN], now_ms: u64) -> Self {
        Self {
            bytes,
            expires_at_ms: now_ms + PAIRING_TTL_MS,
            used: false,
        }
    }

    /// Validate and consume. Expired or reused secrets are rejected.
    pub fn take(&mut self, now_ms: u64) -> Result<[u8; PAIRING_SECRET_LEN], PairingError> {
        if self.used {
            return Err(PairingError::Reused);
        }
        if now_ms > self.expires_at_ms {
            return Err(PairingError::Reused);
        }
        self.used = true;
        Ok(self.bytes)
    }

    pub fn bytes(&self) -> &[u8; PAIRING_SECRET_LEN] {
        &self.bytes
    }

    pub fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }
}

/// Counts failed pairing attempts and enforces lockout. One per pairing
/// server (not per client: an attacker can spoof source addresses on LAN).
#[derive(Debug, Default)]
pub struct AttemptGuard {
    fails: u32,
    locked_until_ms: u64,
}

impl AttemptGuard {
    pub fn check(&self, now_ms: u64) -> Result<(), PairingError> {
        if now_ms < self.locked_until_ms {
            return Err(PairingError::LockedOut);
        }
        Ok(())
    }

    pub fn note_failure(&mut self, now_ms: u64) {
        self.fails += 1;
        if self.fails >= MAX_ATTEMPTS {
            self.locked_until_ms = now_ms + LOCKOUT_MS;
            self.fails = 0;
        }
    }

    pub fn note_success(&mut self) {
        self.fails = 0;
        self.locked_until_ms = 0;
    }
}

/// Data encoded in the desktop QR code. Binary, base64url, no padding:
/// `magic[4] | ver u8 | server_fp[32] | tcp_port u16BE | udp_port u16BE |
///  secret[16] | expires_unix u32BE | name_len u8 | name UTF-8`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrPayload {
    pub server_fp: [u8; 32],
    pub tcp_port: u16,
    pub udp_port: u16,
    pub secret: [u8; PAIRING_SECRET_LEN],
    pub expires_unix: u32,
    pub device_name: String,
}

impl QrPayload {
    pub fn encode(&self) -> Result<String, PairingError> {
        let name = self.device_name.as_bytes();
        if name.len() > 64 {
            return Err(PairingError::NameTooLong);
        }
        let mut raw = Vec::with_capacity(60 + name.len());
        raw.extend_from_slice(QR_MAGIC);
        raw.push(QR_VERSION);
        raw.extend_from_slice(&self.server_fp);
        raw.extend_from_slice(&self.tcp_port.to_be_bytes());
        raw.extend_from_slice(&self.udp_port.to_be_bytes());
        raw.extend_from_slice(&self.secret);
        raw.extend_from_slice(&self.expires_unix.to_be_bytes());
        raw.push(name.len() as u8);
        raw.extend_from_slice(name);
        Ok(URL_SAFE_NO_PAD.encode(raw))
    }

    pub fn decode(text: &str, now_unix: u64) -> Result<Self, PairingError> {
        let raw = URL_SAFE_NO_PAD
            .decode(text.trim())
            .map_err(|_| PairingError::BadQr)?;
        if raw.len() < 4 + 1 + 32 + 2 + 2 + 16 + 4 + 1 {
            return Err(PairingError::BadQr);
        }
        if &raw[..4] != QR_MAGIC || raw[4] != QR_VERSION {
            return Err(PairingError::BadQr);
        }
        let mut server_fp = [0u8; 32];
        server_fp.copy_from_slice(&raw[5..37]);
        let tcp_port = u16::from_be_bytes([raw[37], raw[38]]);
        let udp_port = u16::from_be_bytes([raw[39], raw[40]]);
        let mut secret = [0u8; PAIRING_SECRET_LEN];
        secret.copy_from_slice(&raw[41..57]);
        let expires_unix = u32::from_be_bytes([raw[57], raw[58], raw[59], raw[60]]);
        if u64::from(expires_unix) < now_unix {
            return Err(PairingError::Expired);
        }
        let name_len = raw[61] as usize;
        if raw.len() != 62 + name_len || name_len > 64 {
            return Err(PairingError::BadQr);
        }
        let device_name = String::from_utf8(raw[62..].to_vec()).map_err(|_| PairingError::BadQr)?;
        Ok(Self {
            server_fp,
            tcp_port,
            udp_port,
            secret,
            expires_unix,
            device_name,
        })
    }
}

/// A 6-digit short code (`000000`–`999999`, zero-padded for display).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortCode([u8; 6]);

impl ShortCode {
    /// Random code from OS randomness.
    pub fn generate() -> Result<Self, PairingError> {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).map_err(|_| PairingError::Spake2)?;
        let n = u32::from_le_bytes(b) % 1_000_000;
        Ok(Self::from_number(n))
    }

    pub fn from_number(n: u32) -> Self {
        let s = format!("{:06}", n % 1_000_000);
        let mut digits = [0u8; 6];
        digits.copy_from_slice(s.as_bytes());
        Self(digits)
    }

    pub fn display(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }

    fn password(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

/// SPAKE2 initiator message (phone side, acting as party A).
pub fn spake2_start_a(code: &ShortCode) -> Result<(Spake2<Ed25519Group>, Vec<u8>), PairingError> {
    let (s, msg) = Spake2::<Ed25519Group>::start_a(
        &Password::new(code.password()),
        &Identity::new(b"wpt-phone"),
        &Identity::new(b"wpt-desktop"),
    );
    Ok((s, msg))
}

/// SPAKE2 responder message (desktop side, party B). Identities stay in
/// A,B order on both sides (the responder does NOT swap them).
pub fn spake2_start_b(code: &ShortCode) -> Result<(Spake2<Ed25519Group>, Vec<u8>), PairingError> {
    let (s, msg) = Spake2::<Ed25519Group>::start_b(
        &Password::new(code.password()),
        &Identity::new(b"wpt-phone"),
        &Identity::new(b"wpt-desktop"),
    );
    Ok((s, msg))
}

/// Finish the exchange. Both sides derive the same 32-byte key, which
/// becomes the pairing PSK.
///
/// Note: `finish` cannot detect a wrong code by itself — mismatched sides
/// just derive different keys (per the SPAKE2 security contract, each
/// execution allows one password guess). Confirmation comes from the next
/// step: the key is used as the PSK of the Noise pairing handshake, whose
/// message 3 authentication fails on mismatch. Count Noise failures (not
/// SPAKE2 exchanges) in the [`AttemptGuard`].
pub fn spake2_finish(
    state: Spake2<Ed25519Group>,
    peer_msg: &[u8],
) -> Result<[u8; 32], PairingError> {
    let key = state.finish(peer_msg).map_err(|_| PairingError::Spake2)?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&key[..32.min(key.len())]);
    Ok(out)
}

/// A trusted peer device. Stored by both sides (desktop: JSON + keyring for
/// our own private key; phone: secure storage).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPeer {
    pub name: String,
    pub pubkey: [u8; 32],
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
}

impl TrustedPeer {
    pub fn matches(&self, pubkey: &[u8]) -> bool {
        self.pubkey.as_slice() == pubkey
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_single_use_and_expiry() {
        let mut s = PairingSecret::from_bytes([1u8; 16], 1000);
        assert!(s.take(1000).is_ok());
        assert_eq!(s.take(1001), Err(PairingError::Reused));
        let mut s = PairingSecret::from_bytes([1u8; 16], 1000);
        assert_eq!(s.take(1000 + PAIRING_TTL_MS + 1), Err(PairingError::Reused));
        assert!(PairingSecret::generate(0).is_ok());
    }

    #[test]
    fn guard_locks_out_after_three_failures() {
        let mut g = AttemptGuard::default();
        g.note_failure(0);
        g.note_failure(0);
        assert!(g.check(0).is_ok());
        g.note_failure(0);
        assert_eq!(g.check(0), Err(PairingError::LockedOut));
        assert_eq!(g.check(LOCKOUT_MS - 1), Err(PairingError::LockedOut));
        assert!(g.check(LOCKOUT_MS).is_ok());
        g.note_failure(LOCKOUT_MS);
        g.note_success();
        assert!(g.check(LOCKOUT_MS + 1).is_ok());
    }

    #[test]
    fn qr_round_trip_and_rejects() {
        let p = QrPayload {
            server_fp: [9u8; 32],
            tcp_port: 51516,
            udp_port: 51515,
            secret: [3u8; 16],
            expires_unix: 4_000_000_000,
            device_name: "VIRGIL-PC".to_string(),
        };
        let text = p.encode().expect("encode");
        assert!(!text.contains('+') && !text.contains('/') && !text.contains('='));
        let back = QrPayload::decode(&text, 1_700_000_000).expect("decode");
        assert_eq!(back, p);
        assert_eq!(
            QrPayload::decode(&text, 4_000_000_001),
            Err(PairingError::Expired)
        );
        assert_eq!(
            QrPayload::decode("WPT1bogus!!!", 0),
            Err(PairingError::BadQr)
        );
        let mut bad = QrPayload::decode(&text, 0).expect("decode");
        bad.device_name = "x".repeat(65);
        assert_eq!(bad.encode(), Err(PairingError::NameTooLong));
    }

    #[test]
    fn spake2_same_code_agrees_wrong_code_fails() {
        let code = ShortCode::from_number(123456);
        assert_eq!(code.display(), "123456");
        let (a, msg_a) = spake2_start_a(&code).expect("start a");
        let (b, msg_b) = spake2_start_b(&code).expect("start b");
        let ka = spake2_finish(a, &msg_b).expect("finish a");
        let kb = spake2_finish(b, &msg_a).expect("finish b");
        assert_eq!(ka, kb);

        let wrong = ShortCode::from_number(654321);
        let (a2, _) = spake2_start_a(&wrong).expect("start a2");
        // A wrong code yields a different key, not an error: confirmation
        // happens when the key is used as the Noise PSK (message 3 fails).
        let ka2 = spake2_finish(a2, &msg_b).expect("finish a2");
        assert_ne!(ka, ka2);
    }

    #[test]
    fn short_code_display_zero_padded() {
        assert_eq!(ShortCode::from_number(7).display(), "000007");
        assert_eq!(ShortCode::generate().expect("gen").display().len(), 6);
    }
}
