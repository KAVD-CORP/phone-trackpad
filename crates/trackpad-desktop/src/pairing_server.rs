//! TCP pairing + reconnect server.
//!
//! One thread accepts connections; each connection is handled on its own
//! thread with per-step read timeouts (a stalled peer can never wedge the
//! server). Frames are `u16-BE length + bytes`, capped at 8 KiB.
//!
//! Client hello selects the flow: `PAIR-QR`, `PAIR-SPAKE2`, `RECONNECT`.
//! Pairing ends in a [`PendingApproval`] parked for the UI; the handler
//! thread blocks (bounded) until approve/deny/timeout, then writes the
//! final frame. Reconnect needs no approval: a trusted static key is
//! sufficient, and the session publishes immediately.
//!
//! PSK confirmation subtlety: neither the QR secret nor the SPAKE2 output
//! is "checked" directly — each becomes the PSK of the Noise XXpsk3
//! handshake, whose message-3 authentication fails on mismatch. Pairing
//! attempt accounting ([`AttemptGuard`]) counts Noise failures.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use trackpad_core::crypto::{self, NoiseHandshake, UdpKeys};
use trackpad_core::pairing::{AttemptGuard, PairingError, PairingSecret, ShortCode, TrustedPeer};
use trackpad_core::DEFAULT_TCP_PORT;

const FRAME_MAX: usize = 8192;
const STEP_TIMEOUT: Duration = Duration::from_secs(15);
const APPROVAL_TIMEOUT_MS: u64 = 60_000;
const DEVICE_NAME_MAX: usize = 64;

/// A completed pairing handshake waiting for the human to approve.
/// The handler thread that created it blocks until [`PairingShared`]
/// resolves it (approve/deny/timeout).
#[derive(Debug)]
pub struct PendingApproval {
    pub id: u64,
    pub device_name: String,
    pub peer_pubkey: [u8; 32],
    pub keys: UdpKeys,
    pub created_ms: u64,
    resolved: Option<bool>,
}

/// A live, approved input session published for the UDP loop.
#[derive(Debug, Clone)]
pub struct LiveSession {
    pub peer_pubkey: [u8; 32],
    pub peer_name: String,
    pub keys: UdpKeys,
    pub session_id: u64,
    pub started_ms: u64,
}

/// Active ceremony material shown in the desktop UI.
#[derive(Debug, Clone)]
pub struct Ceremony {
    pub qr_text: String,
    pub qr_svg: String,
    pub short_code: String,
    pub expires_at_ms: u64,
}

/// Mutable server state shared between the TCP threads, the UDP loop,
/// and the UI. Lock briefly; never hold across network I/O.
pub struct PairingShared {
    local_priv: Vec<u8>,
    local_pub: Vec<u8>,
    device_name: String,
    udp_port: u16,
    secret: Option<PairingSecret>,
    short_code: Option<ShortCode>,
    guard: AttemptGuard,
    pending: Vec<PendingApproval>,
    next_approval_id: u64,
    live: Option<LiveSession>,
    trust: crate::trust::TrustStore,
    now_ms: fn() -> u64,
}

impl PairingShared {
    pub fn new(
        local_priv: Vec<u8>,
        local_pub: Vec<u8>,
        device_name: String,
        udp_port: u16,
        trust: crate::trust::TrustStore,
    ) -> Self {
        Self {
            local_priv,
            local_pub,
            device_name,
            udp_port,
            secret: None,
            short_code: None,
            guard: AttemptGuard::default(),
            pending: Vec::new(),
            next_approval_id: 1,
            live: None,
            trust,
            now_ms: crate::now_ms,
        }
    }

    #[cfg(test)]
    fn test(now: u64) -> (Self, Vec<u8>, Vec<u8>) {
        let (priv_key, pub_key) = crypto::generate_keypair().expect("keygen");
        let mut s = Self::new(
            priv_key.clone(),
            pub_key.clone(),
            "test-pc".to_string(),
            51515,
            crate::trust::TrustStore::default(),
        );
        s.now_ms = test_clock;
        TEST_NOW.with(|t| *t.borrow_mut() = now);
        (s, priv_key, pub_key)
    }

    #[cfg(test)]
    fn test_secret(&self) -> [u8; 16] {
        *self.secret.as_ref().expect("ceremony").bytes()
    }

    #[cfg(test)]
    fn test_code(&self) -> ShortCode {
        self.short_code.expect("ceremony")
    }

    fn now(&self) -> u64 {
        (self.now_ms)()
    }

    /// Start (or restart) a ceremony: fresh secret + fresh short code.
    /// Previous material is discarded. Pairing mode is only "on" while a
    /// ceremony exists and is unexpired — checked on every attempt.
    pub fn begin_ceremony(&mut self) -> Result<Ceremony, PairingError> {
        let now = self.now();
        let secret = PairingSecret::generate(now)?;
        let code = ShortCode::generate()?;
        let expires_at_ms = secret.expires_at_ms();
        let server_fp: [u8; 32] = self
            .our_public()
            .try_into()
            .map_err(|_| PairingError::Spake2)?;
        let qr_text = crate::qr::qr_text(
            &server_fp,
            DEFAULT_TCP_PORT,
            self.udp_port,
            &secret,
            &self.device_name,
        )?;
        let qr_svg = crate::qr::qr_svg(&qr_text).map_err(|_| PairingError::Spake2)?;
        self.secret = Some(secret);
        self.short_code = Some(code);
        Ok(Ceremony {
            qr_text,
            qr_svg,
            short_code: code.display(),
            expires_at_ms,
        })
    }

    fn our_public(&self) -> &[u8] {
        &self.local_pub
    }

    pub fn ceremony_active(&self) -> bool {
        self.secret
            .as_ref()
            .is_some_and(|s| self.now() <= s.expires_at_ms())
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Resolve a pending approval. On approve, trust is stored and the live
    /// session publishes; the parked handler thread observes `resolved` and
    /// sends the final frame. Returns false for unknown ids.
    pub fn approve(&mut self, id: u64, approve: bool) -> bool {
        let now = self.now();
        let Some(entry) = self.pending.iter_mut().find(|p| p.id == id) else {
            return false;
        };
        if entry.resolved.is_some() {
            return true; // already decided; the waiter picks it up
        }
        entry.resolved = Some(approve);
        if approve {
            self.trust.add(TrustedPeer {
                name: entry.device_name.clone(),
                pubkey: entry.peer_pubkey,
                first_seen_ms: now,
                last_seen_ms: now,
            });
            let _ = self.trust.save();
            self.secret = None; // single-use ceremony consumed
            let keys = UdpKeys {
                phone_key: entry.keys.phone_key,
                desktop_key: entry.keys.desktop_key,
            };
            self.live = Some(LiveSession {
                peer_pubkey: entry.peer_pubkey,
                peer_name: entry.device_name.clone(),
                keys,
                session_id: now ^ 0x9E37_79B9_7F4A_7C15,
                started_ms: now,
            });
        }
        true
    }

    /// Current live session id for the OK frame (0 when none).
    pub fn live_session_id(&self) -> u64 {
        self.live.as_ref().map(|l| l.session_id).unwrap_or(0)
    }

    pub fn pending(&self) -> &[PendingApproval] {
        &self.pending
    }

    pub fn live(&self) -> Option<&LiveSession> {
        self.live.as_ref()
    }

    /// Revoke a device: removed from trust immediately; the UDP loop drops
    /// its packets on the per-packet trust check, and the live session is
    /// cleared if it belonged to the revoked peer.
    pub fn revoke(&mut self, pubkey: &[u8]) -> bool {
        let removed = self.trust.revoke(pubkey);
        let _ = self.trust.save();
        if self
            .live
            .as_ref()
            .is_some_and(|l| l.peer_pubkey.as_slice() == pubkey)
        {
            self.live = None;
        }
        removed
    }

    pub fn trust_peers(&self) -> &[TrustedPeer] {
        self.trust.peers()
    }
}

#[cfg(test)]
thread_local! {
    static TEST_NOW: std::cell::RefCell<u64> = const { std::cell::RefCell::new(0) };
}

#[cfg(test)]
fn test_clock() -> u64 {
    TEST_NOW.with(|t| *t.borrow())
}

/// Framed-TCP helpers shared with tests.
pub fn write_frame(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    if data.len() > FRAME_MAX {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "frame too large",
        ));
    }
    stream.write_all(&(data.len() as u16).to_be_bytes())?;
    stream.write_all(data)
}

pub fn read_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 2];
    stream.read_exact(&mut len)?;
    let len = u16::from_be_bytes(len) as usize;
    if len > FRAME_MAX {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

/// Handle one TCP connection. Pure state machine over framed I/O; the only
/// blocking-on-human step is the approval park (bounded by
/// [`APPROVAL_TIMEOUT_MS`).
pub fn handle_connection(
    stream: TcpStream,
    shared: Arc<Mutex<PairingShared>>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(STEP_TIMEOUT))?;
    let mut stream = stream;
    let peer: SocketAddr = stream.peer_addr()?;
    let hello = String::from_utf8(read_frame(&mut stream)?)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad hello"))?;
    match hello.as_str() {
        "PAIR-QR" => pair_qr(stream, shared, peer),
        "PAIR-SPAKE2" => pair_spake2(stream, shared, peer),
        "RECONNECT" => reconnect(stream, shared, peer),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unknown hello",
        )),
    }
}

fn active_secret(shared: &Arc<Mutex<PairingShared>>) -> Result<[u8; 16], PairingError> {
    let mut s = shared.lock().expect("pairing lock");
    let now = s.now();
    s.guard.check(now)?;
    let secret = s.secret.as_mut().ok_or(PairingError::Expired)?;
    if now > secret.expires_at_ms() {
        return Err(PairingError::Expired);
    }
    // Copy the bytes without consuming: the ceremony is consumed on
    // approval success, so a dropped connection can retry within the TTL.
    Ok(*secret.bytes())
}

fn xx_exchange(
    stream: &mut TcpStream,
    hs: &mut NoiseHandshake,
    device_name_out: &mut Vec<u8>,
) -> Result<(), PairingError> {
    // Responder flow: read m1, write m2, read m3.
    // (Initiator side lives on the phone; tested in-process in core.)
    let m1 = read_frame(stream).map_err(|_| PairingError::Spake2)?;
    let _ = hs.read_message(&m1).map_err(|_| PairingError::Spake2)?;
    let m2 = hs.write_message(&[]).map_err(|_| PairingError::Spake2)?;
    write_frame(stream, &m2).map_err(|_| PairingError::Spake2)?;
    let m3 = read_frame(stream).map_err(|_| PairingError::Spake2)?;
    let payload = hs.read_message(&m3).map_err(|_| PairingError::Spake2)?;
    *device_name_out = payload;
    Ok(())
}

fn pair_qr(
    mut stream: TcpStream,
    shared: Arc<Mutex<PairingShared>>,
    peer: SocketAddr,
) -> std::io::Result<()> {
    let psk = match active_secret(&shared) {
        Ok(raw) => stretch_psk(&raw),
        Err(e) => {
            let _ = write_frame(&mut stream, format!("ERR {e}").as_bytes());
            fail(&shared);
            return Ok(());
        }
    };
    run_pair_handshake(&mut stream, shared, peer, &psk)
}

fn pair_spake2(
    mut stream: TcpStream,
    shared: Arc<Mutex<PairingShared>>,
    peer: SocketAddr,
) -> std::io::Result<()> {
    let code = {
        let s = shared.lock().expect("pairing lock");
        match s.short_code {
            Some(c) => c,
            None => {
                let _ = write_frame(&mut stream, b"ERR no ceremony");
                return Ok(());
            }
        }
    };
    // SPAKE2 exchange first (2 frames), then the Noise handshake.
    let (state, msg_b) = match trackpad_core::pairing::spake2_start_b(&code) {
        Ok(v) => v,
        Err(e) => {
            let _ = write_frame(&mut stream, format!("ERR {e}").as_bytes());
            return Ok(());
        }
    };
    if write_frame(&mut stream, &msg_b).is_err() {
        fail(&shared);
        return Ok(());
    }
    let msg_a = match read_frame(&mut stream) {
        Ok(m) => m,
        Err(_) => {
            fail(&shared);
            return Ok(());
        }
    };
    let psk = match trackpad_core::pairing::spake2_finish(state, &msg_a) {
        Ok(k) => k,
        Err(e) => {
            let _ = write_frame(&mut stream, format!("ERR {e}").as_bytes());
            fail(&shared);
            return Ok(());
        }
    };
    run_pair_handshake(&mut stream, shared, peer, &psk)
}

fn run_pair_handshake(
    stream: &mut TcpStream,
    shared: Arc<Mutex<PairingShared>>,
    peer: SocketAddr,
    psk: &[u8; 32],
) -> std::io::Result<()> {
    let local_priv = shared.lock().expect("pairing lock").local_priv.clone();
    let mut hs = match NoiseHandshake::responder_pair(&local_priv, psk) {
        Ok(h) => h,
        Err(_) => {
            fail(&shared);
            return Ok(());
        }
    };
    let mut device_name = Vec::new();
    if xx_exchange(stream, &mut hs, &mut device_name).is_err() {
        // Message-3 authentication failed: wrong/expired secret or code.
        // The secret is NOT consumed (single-use means single success);
        // the attempt counter handles guessing.
        fail(&shared);
        let _ = write_frame(stream, b"ERR handshake");
        return Ok(());
    }
    let peer_pubkey = match hs.remote_static() {
        Some(k) if k.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&k);
            arr
        }
        _ => {
            fail(&shared);
            return Ok(());
        }
    };
    let keys = match crypto::derive_udp_keys(&hs.handshake_hash()) {
        Ok(k) => k,
        Err(_) => {
            fail(&shared);
            return Ok(());
        }
    };
    let name = sanitize_name(&device_name);
    let (id, deadline) = {
        let mut s = shared.lock().expect("pairing lock");
        let now = s.now();
        let id = s.next_approval_id;
        s.next_approval_id += 1;
        s.pending.push(PendingApproval {
            id,
            device_name: name.clone(),
            peer_pubkey,
            keys,
            created_ms: now,
            resolved: None,
        });
        (id, now + APPROVAL_TIMEOUT_MS)
    };
    println!("pairing: approval needed for '{name}' from {peer} (id {id})");
    if write_frame(stream, b"PENDING").is_err() {
        remove_pending(&shared, id);
        return Ok(());
    }
    // Park until the UI approves/denies or the deadline passes. If the
    // phone drops the TCP connection mid-wait, its approval card is dead:
    // expire it immediately so it never sits in the UI after the peer vanished.
    let approved = wait_for_approval(&shared, id, deadline, stream);
    let _ = stream;
    if approved {
        let (udp_port, session_id) = {
            let s = shared.lock().expect("pairing lock");
            (s.udp_port, s.live_session_id())
        };
        let _ = write_frame(stream, format!("OK {udp_port} {session_id}").as_bytes());
        succeed(&shared);
        println!("pairing: approved '{name}'");
    } else {
        let _ = write_frame(stream, b"DENIED");
        remove_pending(&shared, id);
        println!("pairing: denied/timeout '{name}'");
    }
    Ok(())
}

fn reconnect(
    mut stream: TcpStream,
    shared: Arc<Mutex<PairingShared>>,
    peer: SocketAddr,
) -> std::io::Result<()> {
    let local_priv = shared.lock().expect("pairing lock").local_priv.clone();
    let mut hs = match NoiseHandshake::responder_reconnect(&local_priv) {
        Ok(h) => h,
        Err(_) => return Ok(()),
    };
    let mut device_name = Vec::new();
    if xx_exchange(&mut stream, &mut hs, &mut device_name).is_err() {
        return Ok(());
    }
    let peer_pubkey = match hs.remote_static() {
        Some(k) if k.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&k);
            arr
        }
        _ => return Ok(()),
    };
    // Trusted static only. No approval, no ceremony.
    let trusted = {
        let mut s = shared.lock().expect("pairing lock");
        let now = s.now();
        if !s.trust.is_trusted(&peer_pubkey) {
            return Ok(());
        }
        let keys = match crypto::derive_udp_keys(&hs.handshake_hash()) {
            Ok(k) => k,
            Err(_) => return Ok(()),
        };
        let name = s
            .trust
            .peers()
            .iter()
            .find(|p| p.matches(&peer_pubkey))
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "phone".to_string());
        s.live = Some(LiveSession {
            peer_pubkey,
            peer_name: name,
            keys,
            session_id: now ^ 0x9E37_79B9_7F4A_7C15,
            started_ms: now,
        });
        (s.udp_port, s.live_session_id())
    };
    println!("reconnect: trusted peer from {peer}, session live");
    let _ = write_frame(
        &mut stream,
        format!("OK {} {}", trusted.0, trusted.1).as_bytes(),
    );
    Ok(())
}

fn wait_for_approval(
    shared: &Arc<Mutex<PairingShared>>,
    id: u64,
    deadline: u64,
    stream: &TcpStream,
) -> bool {
    loop {
        {
            let mut s = shared.lock().expect("pairing lock");
            if let Some(pos) = s.pending.iter().position(|p| p.id == id) {
                if let Some(decision) = s.pending[pos].resolved {
                    s.pending.remove(pos);
                    return decision;
                }
            } else {
                return false; // entry gone without a decision: treat as deny
            }
            if s.now() > deadline {
                s.pending.retain(|p| p.id != id);
                return false;
            }
        }
        // Connection closed by the peer: the pending approval card for a
        // vanished phone must not linger in the desktop UI. A clean EOF
        // (`Ok(0)`) means the phone hung up cleanly — expire its card and
        // drop the handler. Real read errors (incl. WouldBlock on Windows
        // peeks) are ignored; the STEP_TIMEOUT/ deadline still applies.
        if matches!(stream.peek(&mut [0u8; 1]), Ok(0)) {
            remove_pending(shared, id);
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn remove_pending(shared: &Arc<Mutex<PairingShared>>, id: u64) {
    shared
        .lock()
        .expect("pairing lock")
        .pending
        .retain(|p| p.id != id);
}

fn fail(shared: &Arc<Mutex<PairingShared>>) {
    let mut s = shared.lock().expect("pairing lock");
    let now = s.now();
    s.guard.note_failure(now);
}

fn succeed(shared: &Arc<Mutex<PairingShared>>) {
    shared.lock().expect("pairing lock").guard.note_success();
}

/// Stretch a 128-bit QR secret into the 256-bit Noise PSK (HKDF-SHA256,
/// domain-separated; the QR secret has full 128-bit entropy).
pub fn stretch_psk(secret: &[u8; 16]) -> [u8; 32] {
    use hkdf::Hkdf;
    use sha2::Sha256;
    let hk = Hkdf::<Sha256>::new(None, secret);
    let mut out = [0u8; 32];
    hk.expand(b"wpt-pair-psk-v1", &mut out)
        .expect("hkdf expand cannot fail for 32 bytes");
    out
}

/// Device names come from the peer: cap length, strip control characters.
fn sanitize_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let clean: String = s
        .chars()
        .filter(|c| !c.is_control())
        .take(DEVICE_NAME_MAX)
        .collect();
    let clean = clean.trim().to_string();
    if clean.is_empty() {
        "phone".to_string()
    } else {
        clean
    }
}

/// Accept loop. Runs on its own thread; each connection gets a thread.
pub fn serve(listener: TcpListener, shared: Arc<Mutex<PairingShared>>) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, shared) {
                        eprintln!("pairing: connection failed: {e}");
                    }
                });
            }
            Err(e) => eprintln!("pairing: accept failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trackpad_core::crypto::NoiseHandshake;

    /// Test server on loopback with a live ceremony. Returns (shared, addr).
    /// Note: server threads read the fake clock as 0 (thread-local default)
    /// while the test thread sets TEST_NOW. All server-side comparisons are
    /// relative (expiry vs creation, lockout windows), so behavior is exact.
    fn spawn(now: u64) -> (Arc<Mutex<PairingShared>>, SocketAddr) {
        let (state, _, _) = PairingShared::test(now);
        let shared = Arc::new(Mutex::new(state));
        shared
            .lock()
            .expect("lock")
            .begin_ceremony()
            .expect("ceremony");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let s = Arc::clone(&shared);
        std::thread::spawn(move || serve(listener, s));
        // Fake clock stays valid for the TTL; real threads only park.
        (shared, addr)
    }

    fn connect(addr: SocketAddr) -> TcpStream {
        let s = TcpStream::connect(addr).expect("connect");
        s.set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        s
    }

    /// Drive the phone side of a QR pairing. Returns (client_pubkey, frames).
    fn client_pair_qr(
        addr: SocketAddr,
        server_pub: &[u8],
        psk: &[u8; 32],
    ) -> (Vec<u8>, TcpStream, String) {
        let (client_priv, client_pub) = crypto::generate_keypair().expect("keygen");
        let mut hs = NoiseHandshake::initiator_pair(&client_priv, psk).expect("initiator");
        let mut stream = connect(addr);
        write_frame(&mut stream, b"PAIR-QR").expect("hello");
        let m1 = hs.write_message(&[]).expect("m1");
        write_frame(&mut stream, &m1).expect("send m1");
        let m2 = read_frame(&mut stream).expect("read m2");
        hs.read_message(&m2).expect("m2 decodes");
        // Fingerprint check: responder static must equal the QR fingerprint.
        assert_eq!(hs.remote_static().expect("static").as_slice(), server_pub);
        let m3 = hs.write_message(b"test-phone").expect("m3");
        write_frame(&mut stream, &m3).expect("send m3");
        let status = String::from_utf8(read_frame(&mut stream).expect("status")).expect("utf8");
        (client_pub, stream, status)
    }

    #[test]
    fn qr_pair_approve_and_reconnect() {
        let (shared, addr) = spawn(1_000_000);
        let server_pub = shared.lock().expect("lock").local_pub.clone();
        let secret = shared.lock().expect("lock").test_secret();
        let psk = stretch_psk(&secret);

        let (client_pub, mut stream, status) = client_pair_qr(addr, &server_pub, &psk);
        assert_eq!(status, "PENDING");
        // Not trusted yet: reconnect attempts fail silently.
        assert!(!shared
            .lock()
            .expect("lock")
            .trust_peers()
            .iter()
            .any(|p| p.matches(&client_pub)));

        // Approve on another thread (the handler parks waiting).
        let s = Arc::clone(&shared);
        let pending_id = shared.lock().expect("lock").pending()[0].id;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            assert!(s.lock().expect("lock").approve(pending_id, true));
        });
        let done = String::from_utf8(read_frame(&mut stream).expect("final")).expect("utf8");
        assert!(done.starts_with("OK "), "got {done}");
        assert!(shared
            .lock()
            .expect("lock")
            .trust_peers()
            .iter()
            .any(|p| p.matches(&client_pub)));

        // Reconnect must use the SAME key as pairing; that flow is covered
        // by reconnect_flow_with_trusted_key. Here: revoke kills everything.
        assert!(shared.lock().expect("lock").live().is_some());
        assert!(shared.lock().expect("lock").revoke(&client_pub));
        assert!(shared.lock().expect("lock").live().is_none());
        assert!(!shared
            .lock()
            .expect("lock")
            .trust_peers()
            .iter()
            .any(|p| p.matches(&client_pub)));
    }

    #[test]
    fn reconnect_flow_with_trusted_key() {
        let (shared, addr) = spawn(2_000_000);
        let (client_priv, client_pub) = crypto::generate_keypair().expect("keygen");
        // Pre-trust the key (as a prior approved pairing would have).
        shared.lock().expect("lock").trust.add(TrustedPeer {
            name: "test-phone".to_string(),
            pubkey: client_pub.as_slice().try_into().expect("32"),
            first_seen_ms: 0,
            last_seen_ms: 0,
        });
        let mut hs = NoiseHandshake::initiator_reconnect(&client_priv).expect("initiator");
        let mut stream = connect(addr);
        write_frame(&mut stream, b"RECONNECT").expect("hello");
        let m1 = hs.write_message(&[]).expect("m1");
        write_frame(&mut stream, &m1).expect("m1");
        let m2 = read_frame(&mut stream).expect("m2");
        hs.read_message(&m2).expect("m2");
        let m3 = hs.write_message(b"test-phone").expect("m3");
        write_frame(&mut stream, &m3).expect("m3");
        let done = String::from_utf8(read_frame(&mut stream).expect("final")).expect("utf8");
        assert!(done.starts_with("OK "), "got {done}");
        assert!(shared.lock().expect("lock").live().is_some());
    }

    #[test]
    fn untrusted_reconnect_gets_no_session() {
        let (shared, addr) = spawn(3_000_000);
        let (client_priv, _) = crypto::generate_keypair().expect("keygen");
        let mut hs = NoiseHandshake::initiator_reconnect(&client_priv).expect("initiator");
        let mut stream = connect(addr);
        write_frame(&mut stream, b"RECONNECT").expect("hello");
        let m1 = hs.write_message(&[]).expect("m1");
        write_frame(&mut stream, &m1).expect("m1");
        let m2 = read_frame(&mut stream).expect("m2");
        hs.read_message(&m2).expect("m2");
        let m3 = hs.write_message(b"stranger").expect("m3");
        write_frame(&mut stream, &m3).expect("m3");
        // Server drops silently: connection closes, no OK frame.
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("timeout");
        let res = read_frame(&mut stream);
        assert!(res.is_err(), "untrusted peer must get no session");
        assert!(shared.lock().expect("lock").live().is_none());
    }

    #[test]
    fn wrong_secret_fails_and_lockout_kicks_in() {
        let (shared, addr) = spawn(4_000_000);
        let server_pub = shared.lock().expect("lock").local_pub.clone();
        let bad = [0xA5u8; 32];
        for attempt in 0..3 {
            let (_, _, status) = client_pair_qr(addr, &server_pub, &bad);
            assert_eq!(status, "ERR handshake", "attempt {attempt}");
        }
        // Locked out now: the server answers the hello with ERR and closes
        // before any handshake message is accepted.
        let mut stream = connect(addr);
        write_frame(&mut stream, b"PAIR-QR").expect("hello");
        let status = String::from_utf8(read_frame(&mut stream).expect("err")).expect("utf8");
        assert!(status.contains("too many attempts"), "locked: {status}");
        assert!(shared.lock().expect("lock").live().is_none());
    }

    #[test]
    fn spake2_short_code_pairs_end_to_end() {
        let (shared, addr) = spawn(5_000_000);
        let code = shared.lock().expect("lock").test_code();
        let server_pub = shared.lock().expect("lock").local_pub.clone();

        let (client_priv, _) = crypto::generate_keypair().expect("keygen");
        let (spake_a, msg_a) = trackpad_core::pairing::spake2_start_a(&code).expect("spake a");
        let mut stream = connect(addr);
        write_frame(&mut stream, b"PAIR-SPAKE2").expect("hello");
        let msg_b = read_frame(&mut stream).expect("msg_b");
        write_frame(&mut stream, &msg_a).expect("msg_a");
        let psk = trackpad_core::pairing::spake2_finish(spake_a, &msg_b).expect("psk");

        // Same Noise exchange as QR, now keyed by the SPAKE2 output.
        let mut hs = NoiseHandshake::initiator_pair(&client_priv, &psk).expect("initiator");
        let m1 = hs.write_message(&[]).expect("m1");
        write_frame(&mut stream, &m1).expect("m1");
        let m2 = read_frame(&mut stream).expect("m2");
        hs.read_message(&m2).expect("m2");
        assert_eq!(hs.remote_static().expect("static").as_slice(), server_pub);
        let m3 = hs.write_message(b"spake-phone").expect("m3");
        write_frame(&mut stream, &m3).expect("m3");
        let status = String::from_utf8(read_frame(&mut stream).expect("status")).expect("utf8");
        assert_eq!(status, "PENDING");
        let id = shared.lock().expect("lock").pending()[0].id;
        assert!(shared.lock().expect("lock").approve(id, true));
        let done = String::from_utf8(read_frame(&mut stream).expect("final")).expect("utf8");
        assert!(done.starts_with("OK "), "got {done}");
    }

    #[test]
    fn deny_leaves_no_trust_no_session() {
        let (shared, addr) = spawn(6_000_000);
        let server_pub = shared.lock().expect("lock").local_pub.clone();
        let secret = shared.lock().expect("lock").test_secret();
        let psk = stretch_psk(&secret);
        let (client_pub, mut stream, status) = client_pair_qr(addr, &server_pub, &psk);
        assert_eq!(status, "PENDING");
        let id = shared.lock().expect("lock").pending()[0].id;
        assert!(shared.lock().expect("lock").approve(id, false));
        let done = String::from_utf8(read_frame(&mut stream).expect("final")).expect("utf8");
        assert_eq!(done, "DENIED");
        assert!(shared.lock().expect("lock").live().is_none());
        assert!(!shared
            .lock()
            .expect("lock")
            .trust_peers()
            .iter()
            .any(|p| p.matches(&client_pub)));
    }

    #[test]
    fn expired_ceremony_rejects_before_crypto() {
        use trackpad_core::pairing::PAIRING_TTL_MS;
        let (state, _, _) = PairingShared::test(7_000_000);
        let shared = Arc::new(Mutex::new(state));
        shared
            .lock()
            .expect("lock")
            .begin_ceremony()
            .expect("ceremony");
        assert!(active_secret(&shared).is_ok());
        TEST_NOW.with(|t| *t.borrow_mut() = 7_000_000 + PAIRING_TTL_MS + 1);
        assert_eq!(active_secret(&shared), Err(PairingError::Expired));
    }

    #[test]
    fn ceremony_consumed_on_approve() {
        let (shared, addr) = spawn(8_000_000);
        let server_pub = shared.lock().expect("lock").local_pub.clone();
        let secret = shared.lock().expect("lock").test_secret();
        let psk = stretch_psk(&secret);
        let (_, mut stream, status) = client_pair_qr(addr, &server_pub, &psk);
        assert_eq!(status, "PENDING");
        let id = shared.lock().expect("lock").pending()[0].id;
        assert!(shared.lock().expect("lock").approve(id, true));
        assert!(!shared.lock().expect("lock").ceremony_active());
        let done = String::from_utf8(read_frame(&mut stream).expect("final")).expect("utf8");
        assert!(done.starts_with("OK "));
    }
}
