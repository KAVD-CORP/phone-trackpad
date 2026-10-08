//! `trackpad-bridge`: flutter_rust_bridge surface.
//!
//! The phone uses the exact same `trackpad-core` as the desktop: every
//! function here is a thin, blocking wrapper over core types. UDP sockets
//! stay in Dart (`RawDatagramSocket`); only crypto + handshake cross FFI.
//! Results are structs (not `Result`) so Dart gets explicit error strings
//! with zero FFI-error-type uncertainty.
//!
//! Long calls (`pair_*`, blocking on TCP + human approval) run on FRB's
//! blocking pool; pair with a timeout on the Dart side as well.

use flutter_rust_bridge::frb;
use trackpad_core::crypto::{derive_udp_keys, open, seal, NoiseHandshake, SESSION_KEY_LEN};
use trackpad_core::pairing::{PairingError, QrPayload};

/// Output of a pairing/reconnect attempt.
pub struct PairOut {
    pub ok: bool,
    pub error: String,
    pub phone_key: Vec<u8>,
    pub desktop_key: Vec<u8>,
    pub udp_port: u16,
    pub session_hint: u64,
    pub server_name: String,
}

/// Decoded QR for the phone UI (pre-validated, expiry-checked).
pub struct QrOut {
    pub ok: bool,
    pub error: String,
    pub host: String,
    pub tcp_port: u16,
    pub udp_port: u16,
    pub server_fp: Vec<u8>,
    pub device_name: String,
}

/// Opened packet for the phone (PONG handling, control acks).
pub struct OpenOut {
    pub ok: bool,
    pub error: String,
    pub session: u64,
    pub seq: u64,
    pub msg_type: u8,
    /// MOVE/SCROLL: dx. CLICK/DOWN/UP: button. PING/PONG: low 32 bits of ts.
    pub a: i64,
    /// MOVE/SCROLL: dy. PING/PONG: high 32 bits of ts.
    pub b: i64,
}

/// Fresh Noise static keypair for this phone.
pub struct KeypairOut {
    pub private_key: Vec<u8>,
    pub public_key: Vec<u8>,
}

#[frb(sync)]
pub fn bridge_gen_keypair() -> KeypairOut {
    match trackpad_core::crypto::generate_keypair() {
        Ok((private_key, public_key)) => KeypairOut {
            private_key,
            public_key,
        },
        // Keygen failure has no recovery; surface as empty keys (Dart
        // treats empty as failure — keygen cannot fail without a broken
        // platform RNG, in which case pairing is impossible anyway).
        Err(_) => KeypairOut {
            private_key: Vec::new(),
            public_key: Vec::new(),
        },
    }
}

#[frb(sync)]
pub fn bridge_qr_decode(qr_text: String, now_unix: u64) -> QrOut {
    let fail = |e: PairingError| QrOut {
        ok: false,
        error: e.to_string(),
        host: String::new(),
        tcp_port: 0,
        udp_port: 0,
        server_fp: Vec::new(),
        device_name: String::new(),
    };
    let (host, payload) = match qr_text.split_once('#') {
        Some((h, p)) => (h.to_string(), p),
        None => (String::new(), qr_text.as_str()),
    };
    if host.is_empty() {
        return fail(PairingError::BadQr);
    }
    match QrPayload::decode(payload, now_unix) {
        Ok(qr) => QrOut {
            ok: true,
            error: String::new(),
            host,
            tcp_port: qr.tcp_port,
            udp_port: qr.udp_port,
            server_fp: qr.server_fp.to_vec(),
            device_name: qr.device_name,
        },
        Err(e) => fail(e),
    }
}

#[frb(sync)]
pub fn bridge_seal_move(key: Vec<u8>, session: u64, seq: u64, dx: i32, dy: i32) -> Vec<u8> {
    seal_checked(
        &key,
        session,
        seq,
        trackpad_core::Message::Move {
            dx: dx.clamp(-32768, 32767) as i16,
            dy: dy.clamp(-32768, 32767) as i16,
        },
    )
}

#[frb(sync)]
pub fn bridge_seal_click(key: Vec<u8>, session: u64, seq: u64, button: u8) -> Vec<u8> {
    seal_checked(&key, session, seq, trackpad_core::Message::Click { button })
}

#[frb(sync)]
pub fn bridge_seal_scroll(key: Vec<u8>, session: u64, seq: u64, dx: i32, dy: i32) -> Vec<u8> {
    seal_checked(
        &key,
        session,
        seq,
        trackpad_core::Message::Scroll {
            dx: dx.clamp(-32768, 32767) as i16,
            dy: dy.clamp(-32768, 32767) as i16,
        },
    )
}

#[frb(sync)]
pub fn bridge_seal_button(key: Vec<u8>, session: u64, seq: u64, button: u8, down: bool) -> Vec<u8> {
    let msg = if down {
        trackpad_core::Message::ButtonDown { button }
    } else {
        trackpad_core::Message::ButtonUp { button }
    };
    seal_checked(&key, session, seq, msg)
}

#[frb(sync)]
pub fn bridge_seal_ping(key: Vec<u8>, session: u64, seq: u64, timestamp_ms: u64) -> Vec<u8> {
    seal_checked(
        &key,
        session,
        seq,
        trackpad_core::Message::Ping { timestamp_ms },
    )
}

fn seal_checked(key: &[u8], session: u64, seq: u64, msg: trackpad_core::Message) -> Vec<u8> {
    let k: [u8; SESSION_KEY_LEN] = key.try_into().unwrap_or([0u8; SESSION_KEY_LEN]);
    seal(&k, session, seq, msg)
}

#[frb(sync)]
pub fn bridge_open(key: Vec<u8>, packet: Vec<u8>) -> OpenOut {
    let fail = |e: String| OpenOut {
        ok: false,
        error: e,
        session: 0,
        seq: 0,
        msg_type: 0,
        a: 0,
        b: 0,
    };
    let k: [u8; SESSION_KEY_LEN] = match key.try_into() {
        Ok(k) => k,
        Err(_) => return fail("bad key length".to_string()),
    };
    let (session, seq, msg) = match open(&k, &packet) {
        Ok(v) => v,
        Err(e) => return fail(e.to_string()),
    };
    let (msg_type, a, b) = match msg {
        trackpad_core::Message::Move { dx, dy } => (0x01, dx as i64, dy as i64),
        trackpad_core::Message::Click { button } => (0x02, button as i64, 0),
        trackpad_core::Message::Ping { timestamp_ms } => (
            0x03,
            (timestamp_ms & 0xFFFF_FFFF) as i64,
            (timestamp_ms >> 32) as i64,
        ),
        trackpad_core::Message::Pong { timestamp_ms } => (
            0x04,
            (timestamp_ms & 0xFFFF_FFFF) as i64,
            (timestamp_ms >> 32) as i64,
        ),
        trackpad_core::Message::ButtonDown { button } => (0x05, button as i64, 0),
        trackpad_core::Message::ButtonUp { button } => (0x06, button as i64, 0),
        trackpad_core::Message::Scroll { dx, dy } => (0x07, dx as i64, dy as i64),
    };
    OpenOut {
        ok: true,
        error: String::new(),
        session,
        seq,
        msg_type,
        a,
        b,
    }
}

/// Stub kept for the Phase 0/1 API shape; prefer the FRB functions above.
pub fn bridge_hello() -> String {
    format!("bridge -> {}", trackpad_core::hello())
}

/// Blocking QR ceremony: TCP + Noise XXpsk3 + fingerprint check, then wait
/// for the desktop approval (server closes on deny/timeout). Returns session
/// keys on `OK`. `client_priv` is this phone's long-term static key —
/// generate once via [`bridge_gen_keypair`], keep in secure storage, and
/// reuse for every pairing and reconnect. Runs on FRB's blocking pool;
/// also bound it on the Dart side.
#[frb(sync)]
pub fn bridge_pair_qr(
    qr_text: String,
    tcp_host: String,
    device_name: String,
    client_priv: Vec<u8>,
) -> PairOut {
    let fail = |error: String| PairOut {
        ok: false,
        error,
        phone_key: Vec::new(),
        desktop_key: Vec::new(),
        udp_port: 0,
        session_hint: 0,
        server_name: String::new(),
    };
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let qr = match QrPayload::decode(
        qr_text.split_once('#').map(|(_, p)| p).unwrap_or(&qr_text),
        now_unix,
    ) {
        Ok(q) => q,
        Err(e) => return fail(e.to_string()),
    };
    let host = qr_text
        .split_once('#')
        .map(|(h, _)| h)
        .filter(|h| !h.is_empty())
        .unwrap_or(&tcp_host);
    let mut psk = [0u8; 32];
    stretch_into(&qr.secret, &mut psk);
    pair_with_psk(
        PairInput {
            host,
            tcp_port: qr.tcp_port,
            psk: &psk,
            expect_fp: &qr.server_fp,
            server_name: &qr.device_name,
            device_name: &device_name,
            client_priv: &client_priv,
        },
        fail,
    )
}

/// Blocking short-code ceremony: SPAKE2 first, then the same Noise
/// handshake keyed by the SPAKE2 output.
#[frb(sync)]
pub fn bridge_pair_code(
    host: String,
    tcp_port: u16,
    code: String,
    server_fp: Vec<u8>,
    server_name: String,
    device_name: String,
    client_priv: Vec<u8>,
) -> PairOut {
    let fail = |error: String| PairOut {
        ok: false,
        error,
        phone_key: Vec::new(),
        desktop_key: Vec::new(),
        udp_port: 0,
        session_hint: 0,
        server_name: String::new(),
    };
    let digits = code.trim();
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return fail("code must be 6 digits".to_string());
    }
    let n: u32 = digits.parse().unwrap_or(1_000_000);
    if n >= 1_000_000 {
        return fail("code must be 6 digits".to_string());
    }
    let sc = trackpad_core::pairing::ShortCode::from_number(n);
    let (state, msg_a) = match trackpad_core::pairing::spake2_start_a(&sc) {
        Ok(v) => v,
        Err(e) => return fail(e.to_string()),
    };
    let mut stream = match connect_tcp(&host, tcp_port) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    if write_frame(&mut stream, b"PAIR-SPAKE2").is_err() {
        return fail("hello failed".to_string());
    }
    let msg_b = match read_frame(&mut stream) {
        Ok(m) => m,
        Err(_) => return fail("no SPAKE2 reply".to_string()),
    };
    if write_frame(&mut stream, &msg_a).is_err() {
        return fail("SPAKE2 send failed".to_string());
    }
    let psk = match trackpad_core::pairing::spake2_finish(state, &msg_b) {
        Ok(k) => k,
        Err(e) => return fail(e.to_string()),
    };
    let fp: [u8; 32] = match server_fp.try_into() {
        Ok(f) => f,
        Err(_) => return fail("bad fingerprint".to_string()),
    };
    pair_noise(
        &mut stream,
        PairInput {
            host: "",
            tcp_port: 0,
            psk: &psk,
            expect_fp: &fp,
            server_name: &server_name,
            device_name: &device_name,
            client_priv: &client_priv,
        },
        fail,
    )
}

/// Blocking reconnect with the stored keypair. No approval, no ceremony.
#[frb(sync)]
pub fn bridge_reconnect(
    host: String,
    tcp_port: u16,
    client_priv: Vec<u8>,
    server_fp: Vec<u8>,
    server_name: String,
    device_name: String,
) -> PairOut {
    let fail = |error: String| PairOut {
        ok: false,
        error,
        phone_key: Vec::new(),
        desktop_key: Vec::new(),
        udp_port: 0,
        session_hint: 0,
        server_name: String::new(),
    };
    let fp: [u8; 32] = match server_fp.try_into() {
        Ok(f) => f,
        Err(_) => return fail("bad fingerprint".to_string()),
    };
    let mut hs = match NoiseHandshake::initiator_reconnect(&client_priv) {
        Ok(h) => h,
        Err(_) => return fail("handshake init failed".to_string()),
    };
    let mut stream = match connect_tcp(&host, tcp_port) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    if write_frame(&mut stream, b"RECONNECT").is_err() {
        return fail("hello failed".to_string());
    }
    match exchange_initiator(&mut stream, &mut hs, &device_name) {
        Ok(_) => {}
        Err(e) => return fail(e),
    }
    if hs.remote_static().as_deref() != Some(fp.as_slice()) {
        return fail("server fingerprint mismatch".to_string());
    }
    finish_ok(&mut stream, &hs, &server_name, fail)
}

fn stretch_into(secret: &[u8; 16], out: &mut [u8; 32]) {
    use hkdf::Hkdf;
    use sha2::Sha256;
    let hk = Hkdf::<Sha256>::new(None, secret);
    hk.expand(b"wpt-pair-psk-v1", out)
        .expect("hkdf expand cannot fail for 32 bytes");
}

fn connect_tcp(host: &str, port: u16) -> Result<std::net::TcpStream, String> {
    use std::net::ToSocketAddrs;
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|_| "unresolvable address".to_string())?
        .next()
        .ok_or_else(|| "unresolvable address".to_string())?;
    let stream = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(10))
        .map_err(|e| format!("connect failed: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(75)))
        .map_err(|e| format!("timeout failed: {e}"))?;
    Ok(stream)
}

fn write_frame(stream: &mut std::net::TcpStream, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    stream.write_all(&(data.len() as u16).to_be_bytes())?;
    stream.write_all(data)
}

fn read_frame(stream: &mut std::net::TcpStream) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut len = [0u8; 2];
    stream.read_exact(&mut len)?;
    let len = u16::from_be_bytes(len) as usize;
    if len > 8192 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

/// Shared initiator exchange: m1 -> read m2 -> m3. Returns after m3 is sent.
fn exchange_initiator(
    stream: &mut std::net::TcpStream,
    hs: &mut NoiseHandshake,
    device_name: &str,
) -> Result<(), String> {
    let m1 = hs.write_message(&[]).map_err(|_| "m1 failed".to_string())?;
    write_frame(stream, &m1).map_err(|_| "m1 send failed".to_string())?;
    let m2 = read_frame(stream).map_err(|_| "m2 missing".to_string())?;
    if m2.starts_with(b"ERR") {
        return Err(String::from_utf8_lossy(&m2).into_owned());
    }
    hs.read_message(&m2)
        .map_err(|_| "m2 auth failed".to_string())?;
    let m3 = hs
        .write_message(device_name.as_bytes())
        .map_err(|_| "m3 failed".to_string())?;
    write_frame(stream, &m3).map_err(|_| "m3 send failed".to_string())?;
    Ok(())
}

/// After m3: pairing waits for PENDING then OK/DENIED; reconnect reads OK.
fn finish_ok(
    stream: &mut std::net::TcpStream,
    hs: &NoiseHandshake,
    server_name: &str,
    fail: impl FnOnce(String) -> PairOut,
) -> PairOut {
    let first = match read_frame(stream) {
        Ok(f) => f,
        Err(_) => return fail("no server reply".to_string()),
    };
    if first.starts_with(b"ERR") {
        return fail(String::from_utf8_lossy(&first).into_owned());
    }
    let ok_line = if first == b"PENDING" {
        match read_frame(stream) {
            Ok(f) => f,
            Err(_) => return fail("approval wait failed".to_string()),
        }
    } else {
        first
    };
    let text = String::from_utf8_lossy(&ok_line);
    if text == "DENIED" || text.starts_with("ERR") {
        return fail(text.into_owned());
    }
    let mut parts = text.split_whitespace();
    if parts.next() != Some("OK") {
        return fail(format!("bad reply: {text}"));
    }
    let udp_port: u16 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let session_hint: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    if udp_port == 0 {
        return fail("bad reply".to_string());
    }
    let keys = match derive_udp_keys(&hs.handshake_hash()) {
        Ok(k) => k,
        Err(_) => return fail("key derivation failed".to_string()),
    };
    PairOut {
        ok: true,
        error: String::new(),
        phone_key: keys.phone_key.to_vec(),
        desktop_key: keys.desktop_key.to_vec(),
        udp_port,
        session_hint,
        server_name: server_name.to_string(),
    }
}

/// Inputs to a Noise pairing exchange (keeps arg counts clipped).
struct PairInput<'a> {
    host: &'a str,
    tcp_port: u16,
    psk: &'a [u8; 32],
    expect_fp: &'a [u8; 32],
    server_name: &'a str,
    device_name: &'a str,
    client_priv: &'a [u8],
}

fn pair_with_psk(input: PairInput<'_>, fail: impl FnOnce(String) -> PairOut) -> PairOut {
    let mut hs = match NoiseHandshake::initiator_pair(input.client_priv, input.psk) {
        Ok(h) => h,
        Err(_) => return fail("handshake init failed".to_string()),
    };
    let mut stream = match connect_tcp(input.host, input.tcp_port) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    if write_frame(&mut stream, b"PAIR-QR").is_err() {
        return fail("hello failed".to_string());
    }
    if let Err(e) = exchange_initiator(&mut stream, &mut hs, input.device_name) {
        return fail(e);
    }
    if hs.remote_static().as_deref() != Some(input.expect_fp.as_slice()) {
        return fail("server fingerprint mismatch".to_string());
    }
    finish_ok(&mut stream, &hs, input.server_name, fail)
}

fn pair_noise(
    stream: &mut std::net::TcpStream,
    input: PairInput<'_>,
    fail: impl FnOnce(String) -> PairOut,
) -> PairOut {
    let mut hs = match NoiseHandshake::initiator_pair(input.client_priv, input.psk) {
        Ok(h) => h,
        Err(_) => return fail("handshake init failed".to_string()),
    };
    if let Err(e) = exchange_initiator(stream, &mut hs, input.device_name) {
        return fail(e);
    }
    if hs.remote_static().as_deref() != Some(input.expect_fp.as_slice()) {
        return fail("server fingerprint mismatch".to_string());
    }
    finish_ok(stream, &hs, input.server_name, fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_links_core() {
        assert!(bridge_hello().contains("trackpad-core hello"));
    }

    #[test]
    fn seal_open_bridge_round_trip() {
        let keys = derive_udp_keys(b"test-handshake-hash-seed").expect("keys");
        let pkt = bridge_seal_move(keys.phone_key.to_vec(), 5, 9, 100, -50);
        assert!(!pkt.is_empty());
        let out = bridge_open(keys.phone_key.to_vec(), pkt);
        assert!(out.ok, "open failed: {}", out.error);
        assert_eq!((out.session, out.seq, out.msg_type), (5, 9, 0x01));
        assert_eq!((out.a, out.b), (100, -50));
        let bad = bridge_open(
            keys.desktop_key.to_vec(),
            bridge_seal_move(keys.phone_key.to_vec(), 5, 9, 1, 1),
        );
        assert!(!bad.ok);
    }

    #[test]
    fn qr_decode_bridge_validates() {
        let out = bridge_qr_decode("not-a-qr".to_string(), 0);
        assert!(!out.ok);
    }

    #[test]
    fn keypair_bridge_generates() {
        let kp = bridge_gen_keypair();
        assert_eq!(kp.private_key.len(), 32);
        assert_eq!(kp.public_key.len(), 32);
    }
}
