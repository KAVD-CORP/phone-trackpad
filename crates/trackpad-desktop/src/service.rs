//! Phase 2 UDP service loop.
//!
//! Invariants:
//! - MOVE / SCROLL are lossy and coalesced: deltas arriving within one
//!   injection tick are summed and applied once, newest wins on loss.
//! - Button events are critical: a pending MOVE/SCROLL batch is flushed
//!   *before* any button event so drag order (DOWN, moves, UP) is kept.
//! - [`ButtonGuard`] tracks held buttons and releases them on session
//!   change or after [`SILENCE_TIMEOUT_MS`] without packets, so a dropped
//!   BUTTON_UP or a dead phone can never leave the mouse stuck down.
//! - Malformed packets are logged and ignored, never fatal.

use crate::InputInjector;
use trackpad_core::crypto::{open, seal, ReplayWindow, UdpKeys};
use trackpad_core::{button, decode, encode, DecodeError, Message};

/// Injection tick: coalesced deltas flush at most this often.
pub const TICK_MS: u64 = 8;
/// Silence after which held buttons are force-released.
pub const SILENCE_TIMEOUT_MS: u64 = 500;

/// Result of handling one datagram: an optional reply to send back.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// MOVE/CLICK applied (or PONG/unknown-button ignored). No reply.
    Applied,
    /// PING received; send these bytes back to the sender.
    Reply(Vec<u8>),
}

/// Sums MOVE/SCROLL deltas between injection ticks. Pure; the service owns
/// one and flushes it on every tick and before each button event.
#[derive(Debug, Default)]
pub struct Coalescer {
    move_dx: i64,
    move_dy: i64,
    scroll_dx: i64,
    scroll_dy: i64,
}

impl Coalescer {
    pub fn push_move(&mut self, dx: i16, dy: i16) {
        self.move_dx += dx as i64;
        self.move_dy += dy as i64;
    }

    pub fn push_scroll(&mut self, dx: i16, dy: i16) {
        self.scroll_dx += dx as i64;
        self.scroll_dy += dy as i64;
    }

    pub fn is_empty(&self) -> bool {
        self.move_dx == 0 && self.move_dy == 0 && self.scroll_dx == 0 && self.scroll_dy == 0
    }

    /// Drain the batch, saturating to `i32` (batches are tiny in practice).
    pub fn take(&mut self) -> ((i32, i32), (i32, i32)) {
        let mv = (
            self.move_dx.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            self.move_dy.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        );
        let sc = (
            self.scroll_dx.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            self.scroll_dy.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        );
        self.move_dx = 0;
        self.move_dy = 0;
        self.scroll_dx = 0;
        self.scroll_dy = 0;
        (mv, sc)
    }
}

/// Tracks held buttons with millisecond timestamps (caller clock, so tests
/// use fake time). All decisions are pure; the service performs the releases.
#[derive(Debug, Default)]
pub struct ButtonGuard {
    held: [bool; 3],
    last_activity_ms: u64,
}

impl ButtonGuard {
    /// A new session started: forget old state, return previously held
    /// buttons so the caller can release them immediately.
    pub fn new_session(&mut self, now_ms: u64) -> Vec<u8> {
        self.last_activity_ms = now_ms;
        self.take_held()
    }

    pub fn note_down(&mut self, b: u8, now_ms: u64) {
        self.last_activity_ms = now_ms;
        if (b as usize) < self.held.len() {
            self.held[b as usize] = true;
        }
    }

    pub fn note_up(&mut self, b: u8, now_ms: u64) {
        self.last_activity_ms = now_ms;
        if (b as usize) < self.held.len() {
            self.held[b as usize] = false;
        }
    }

    pub fn note_activity(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
    }

    /// Buttons to release because the session went silent. Idempotent:
    /// each button is returned at most once until pressed again.
    pub fn releases_due(&mut self, now_ms: u64) -> Vec<u8> {
        if now_ms.saturating_sub(self.last_activity_ms) >= SILENCE_TIMEOUT_MS {
            self.take_held()
        } else {
            Vec::new()
        }
    }

    fn take_held(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for (i, h) in self.held.iter_mut().enumerate() {
            if *h {
                *h = false;
                out.push(i as u8);
            }
        }
        out
    }
}

/// Per-process session state: one phone controls at a time in Phase 2.
/// A new `session_id` means reconnect (or a different phone): held buttons
/// are released and the coalescer is dropped, never carried across.
pub struct SessionState {
    pub session_id: Option<u64>,
    pub coalescer: Coalescer,
    pub guard: ButtonGuard,
    secure: Option<SecureChannel>,
    /// Diagnostics: accepted vs dropped-secure datagrams.
    pub rx_ok: u64,
    pub rx_drop: u64,
}

/// Encrypted input channel bound to one approved pairing session.
struct SecureChannel {
    peer: [u8; 32],
    keys: UdpKeys,
    replay: ReplayWindow,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            session_id: None,
            coalescer: Coalescer::default(),
            guard: ButtonGuard::default(),
            secure: None,
            rx_ok: 0,
            rx_drop: 0,
        }
    }

    /// Bind the encrypted channel after approval/reconnect. Releases any
    /// buttons held by a previous session first: keys changed, so in-flight
    /// state from before is untrustworthy.
    pub fn set_secure(
        &mut self,
        peer: [u8; 32],
        keys: UdpKeys,
        injector: &mut (dyn InputInjector + '_),
    ) {
        self.flush(injector);
        for b in [button::LEFT, button::RIGHT, button::MIDDLE] {
            injector.button_up(b);
        }
        self.coalescer = Coalescer::default();
        self.secure = Some(SecureChannel {
            peer,
            keys,
            replay: ReplayWindow::new(),
        });
    }

    /// Tear down the encrypted channel (revoke, new pairing, shutdown).
    pub fn clear_secure(&mut self, injector: &mut (dyn InputInjector + '_)) {
        self.flush(injector);
        for b in [button::LEFT, button::RIGHT, button::MIDDLE] {
            injector.button_up(b);
        }
        self.coalescer = Coalescer::default();
        self.secure = None;
    }

    pub fn secure_peer(&self) -> Option<&[u8; 32]> {
        self.secure.as_ref().map(|s| &s.peer)
    }

    /// Handle one decoded packet. `now_ms` is the caller's clock.
    /// Returns an optional PONG reply.
    pub fn on_packet(
        &mut self,
        session_id: u64,
        msg: Message,
        now_ms: u64,
        injector: &mut (dyn InputInjector + '_),
    ) -> Option<Vec<u8>> {
        if self.session_id != Some(session_id) {
            for b in self.guard.new_session(now_ms) {
                injector.button_up(b);
            }
            self.coalescer = Coalescer::default();
            self.session_id = Some(session_id);
        }
        match msg {
            Message::Move { dx, dy } => {
                self.guard.note_activity(now_ms);
                self.coalescer.push_move(dx, dy);
                None
            }
            Message::Scroll { dx, dy } => {
                self.guard.note_activity(now_ms);
                self.coalescer.push_scroll(dx, dy);
                None
            }
            Message::Ping { timestamp_ms } => {
                self.guard.note_activity(now_ms);
                self.flush(injector);
                Some(encode(session_id, 0, Message::Pong { timestamp_ms }))
            }
            Message::Click { button: b } => {
                self.guard.note_activity(now_ms);
                self.flush(injector);
                super::dispatch(Message::Click { button: b }, injector);
                None
            }
            Message::ButtonDown { button: b } => {
                self.guard.note_down(b, now_ms);
                self.flush(injector);
                if matches!(b, button::LEFT | button::RIGHT | button::MIDDLE) {
                    injector.button_down(b);
                }
                None
            }
            Message::ButtonUp { button: b } => {
                self.guard.note_up(b, now_ms);
                self.flush(injector);
                injector.button_up(b);
                None
            }
            Message::Pong { .. } => {
                self.guard.note_activity(now_ms);
                None
            }
        }
    }

    /// Handle one v2 encrypted datagram. `trusted` must reflect the live
    /// trust store for the channel peer (revoke takes effect on the next
    /// packet). Returns an encrypted PONG reply when the packet was a PING.
    /// Anything unauthenticated, replayed, or untrusted is dropped
    /// silently and counted in `rx_drop`.
    pub fn on_secure_datagram(
        &mut self,
        data: &[u8],
        trusted: bool,
        now_ms: u64,
        injector: &mut (dyn InputInjector + '_),
    ) -> Option<Vec<u8>> {
        let (phone_key, desktop_key) = match self.secure.as_ref() {
            Some(s) => (s.keys.phone_key, s.keys.desktop_key),
            None => return None,
        };
        if !trusted {
            self.rx_drop += 1;
            return None;
        }
        let (session_id, seq, msg) = match open(&phone_key, data) {
            Ok(v) => v,
            Err(_) => {
                self.rx_drop += 1;
                return None;
            }
        };
        let fresh = self
            .secure
            .as_mut()
            .expect("channel checked above")
            .replay
            .check(seq);
        if !fresh {
            self.rx_drop += 1;
            return None;
        }
        self.rx_ok += 1;
        match msg {
            Message::Ping { timestamp_ms } => {
                self.guard.note_activity(now_ms);
                Some(seal(
                    &desktop_key,
                    session_id,
                    seq,
                    Message::Pong { timestamp_ms },
                ))
            }
            _ => {
                // Reuse the plaintext session machine for ordering/coalescing.
                self.on_packet(session_id, msg, now_ms, injector);
                None
            }
        }
    }

    /// Apply the coalesced batch, if any.
    pub fn flush(&mut self, injector: &mut (dyn InputInjector + '_)) {
        if self.coalescer.is_empty() {
            return;
        }
        let ((mdx, mdy), (sdx, sdy)) = self.coalescer.take();
        if mdx != 0 || mdy != 0 {
            injector.move_relative(mdx, mdy);
        }
        if sdx != 0 || sdy != 0 {
            injector.scroll(sdx, sdy);
        }
    }

    /// Enforce the silence timeout. Call on every tick. Returns released
    /// buttons so the service can log them.
    pub fn check_silence(
        &mut self,
        now_ms: u64,
        injector: &mut (dyn InputInjector + '_),
    ) -> Vec<u8> {
        let mut released = Vec::new();
        for b in self.guard.releases_due(now_ms) {
            injector.button_up(b);
            released.push(b);
        }
        released
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

/// Decode `data` and route it through `state`. Thin wrapper kept for the
/// Phase 1 call shape; new code should use [`SessionState`] directly.
pub fn handle_datagram(
    data: &[u8],
    session: &mut u64,
    injector: &mut (dyn InputInjector + '_),
) -> Result<Outcome, DecodeError> {
    let packet = decode(data)?;
    *session = packet.session_id;
    match packet.msg {
        Message::Ping { timestamp_ms } => {
            let reply = encode(
                packet.session_id,
                packet.seq,
                Message::Pong { timestamp_ms },
            );
            Ok(Outcome::Reply(reply))
        }
        msg => {
            super::dispatch(msg, injector);
            Ok(Outcome::Applied)
        }
    }
}

use std::sync::{Arc, Mutex};
use trackpad_core::crypto::ENCRYPTED_VERSION;
use trackpad_core::{DEFAULT_UDP_PORT, MAX_PACKET_LEN};

/// Lowercase hex for public-key display (approve/revoke UX).
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Parse 64 lowercase/uppercase hex chars into a 32-byte key.
pub fn from_hex(s: &str) -> Result<[u8; 32], &'static str> {
    if s.len() != 64 {
        return Err("need 64 hex chars");
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16).ok_or("bad hex digit")?;
        let lo = (chunk[1] as char).to_digit(16).ok_or("bad hex digit")?;
        out[i] = (hi as u8) * 16 + lo as u8;
    }
    Ok(out)
}

/// The UDP input loop, shared by the standalone binary and the Tauri
/// backend. Never returns; logs to stderr, never panics on network input.
/// `open` allows plaintext v1 packets (LAN testing only; default drops).
pub fn udp_loop(bind: &str, shared: Arc<Mutex<crate::pairing_server::PairingShared>>, open: bool) {
    use std::net::UdpSocket;
    let socket = match UdpSocket::bind(bind) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("trackpad-service: cannot bind {bind}: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = socket.set_read_timeout(Some(std::time::Duration::from_millis(TICK_MS))) {
        eprintln!("trackpad-service: cannot set timeout: {e}");
        std::process::exit(1);
    }
    println!(
        "trackpad-service: UDP on {bind} (secure only{})",
        if open { " + --open plaintext" } else { "" }
    );

    #[cfg(windows)]
    let mut injector = crate::WindowsInjector::default();
    #[cfg(not(windows))]
    let mut injector = crate::MockInjector::default();

    let mut buf = vec![0u8; MAX_PACKET_LEN + 64];
    let mut state = SessionState::new();
    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, src)) => {
                if len == 0 {
                    continue;
                }
                if buf[0] == ENCRYPTED_VERSION {
                    // Sync channel to the live session first (new approval
                    // or revoke while idle).
                    sync_secure(&mut state, &shared, &mut injector);
                    let trusted = trusted_current(&shared, &state);
                    let reply = state.on_secure_datagram(
                        &buf[..len],
                        trusted,
                        crate::now_ms(),
                        &mut injector,
                    );
                    if let Some(reply) = reply {
                        if let Err(e) = socket.send_to(&reply, src) {
                            eprintln!("trackpad-service: PONG send to {src} failed: {e}");
                        }
                    }
                } else if open {
                    match decode(&buf[..len]) {
                        Ok(packet) => {
                            // Plaintext path shares the session machine but
                            // never touches the secure channel.
                            state.session_id = Some(packet.session_id);
                            state.on_packet(
                                packet.session_id,
                                packet.msg,
                                crate::now_ms(),
                                &mut injector,
                            );
                        }
                        Err(e) => {
                            eprintln!("trackpad-service: dropped malformed packet from {src}: {e}")
                        }
                    }
                } else {
                    state.rx_drop += 1;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                eprintln!("trackpad-service: recv error: {e}");
                continue;
            }
        }
        sync_secure(&mut state, &shared, &mut injector);
        state.flush(&mut injector);
        for b in state.check_silence(crate::now_ms(), &mut injector) {
            eprintln!("trackpad-service: released stuck button {b} after silence");
        }
    }
}

/// Reconcile the UDP secure channel with the pairing state: new approval or
/// reconnect publishes keys; revoke or expiry clears them (releasing any
/// held buttons via the injector).
pub fn sync_secure(
    state: &mut SessionState,
    shared: &Arc<Mutex<crate::pairing_server::PairingShared>>,
    injector: &mut (dyn InputInjector + '_),
) {
    enum Op {
        None,
        Set([u8; 32], UdpKeys),
        Clear,
    }
    let op = {
        let s = shared.lock().expect("lock");
        match s.live() {
            Some(live) if s.trust_peers().iter().any(|p| p.matches(&live.peer_pubkey)) => {
                if state.secure_peer() == Some(&live.peer_pubkey) {
                    Op::None
                } else {
                    Op::Set(live.peer_pubkey, live.keys.clone())
                }
            }
            _ => {
                if state.secure_peer().is_some() {
                    Op::Clear
                } else {
                    Op::None
                }
            }
        }
    };
    match op {
        Op::None => {}
        Op::Set(peer, keys) => state.set_secure(peer, keys, injector),
        Op::Clear => state.clear_secure(injector),
    }
}

/// Per-packet trust: the channel peer must still be trusted AND live.
pub fn trusted_current(
    shared: &Arc<Mutex<crate::pairing_server::PairingShared>>,
    state: &SessionState,
) -> bool {
    let s = shared.lock().expect("lock");
    match (state.secure_peer(), s.live()) {
        (Some(cur), Some(live)) => {
            live.peer_pubkey == *cur && s.trust_peers().iter().any(|p| p.matches(cur))
        }
        _ => false,
    }
}

/// Bind address helper: `0.0.0.0:{DEFAULT_UDP_PORT}` unless overridden.
pub fn default_bind() -> String {
    format!("0.0.0.0:{DEFAULT_UDP_PORT}")
}

/// Embedded backend: everything the binary boots, for the Tauri shell.
/// Holds the mDNS advertisement for the process lifetime.
pub struct Backend {
    pub shared: Arc<Mutex<crate::pairing_server::PairingShared>>,
    pub web: Arc<Mutex<crate::web::WebState>>,
    _advertiser: Option<crate::discovery::Advertiser>,
}

/// Boot identity, pairing TCP server, mDNS, and the UDP input loop.
/// Never panics on network errors; logs to stderr and continues degraded.
pub fn boot(udp_port: u16, tcp_port: u16, mdns: bool) -> Backend {
    let identity = crate::trust::DeviceIdentity::load_or_create().unwrap_or_else(|e| {
        eprintln!("trackpad: identity failed: {e}");
        std::process::exit(1);
    });
    let trust = crate::trust::TrustStore::load().unwrap_or_else(|e| {
        eprintln!("trackpad: trust store failed: {e}");
        std::process::exit(1);
    });
    let fp: [u8; 32] = identity
        .public_key
        .as_slice()
        .try_into()
        .expect("stored public key is 32 bytes");
    let shared = Arc::new(Mutex::new(crate::pairing_server::PairingShared::new(
        identity.private_key,
        identity.public_key,
        identity.device_name.clone(),
        udp_port,
        trust,
    )));
    if let Ok(listener) = std::net::TcpListener::bind(format!("0.0.0.0:{tcp_port}")) {
        let tcp_shared = Arc::clone(&shared);
        std::thread::spawn(move || crate::pairing_server::serve(listener, tcp_shared));
    } else {
        eprintln!("trackpad: TCP {tcp_port} busy (another instance?)");
    }
    let advertiser = if mdns {
        match crate::discovery::Advertiser::start(&identity.device_name, &fp, tcp_port, udp_port) {
            Ok(a) => {
                println!("trackpad: advertising {}", a.fullname());
                Some(a)
            }
            Err(e) => {
                eprintln!("trackpad: mDNS failed (continuing): {e}");
                None
            }
        }
    } else {
        None
    };
    let udp_shared = Arc::clone(&shared);
    let bind = format!("0.0.0.0:{udp_port}");
    std::thread::spawn(move || udp_loop(&bind, udp_shared, false));
    let web = Arc::new(Mutex::new(crate::web::WebState::new()));
    if let Ok(listener) = std::net::TcpListener::bind(format!("0.0.0.0:{}", crate::web::WEB_PORT)) {
        let web_shared = Arc::clone(&web);
        std::thread::spawn(move || crate::web::serve(listener, web_shared));
    } else {
        eprintln!("trackpad: web port {} busy", crate::web::WEB_PORT);
    }
    Backend {
        shared,
        web,
        _advertiser: advertiser,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockInjector;
    use trackpad_core::BUTTON_LEFT;

    #[test]
    fn coalescer_sums_and_drains() {
        let mut c = Coalescer::default();
        assert!(c.is_empty());
        c.push_move(5, -3);
        c.push_move(-2, 1);
        c.push_scroll(0, 80);
        assert!(!c.is_empty());
        assert_eq!(c.take(), ((3, -2), (0, 80)));
        assert!(c.is_empty());
    }

    #[test]
    fn guard_releases_on_silence_once() {
        let mut g = ButtonGuard::default();
        g.note_down(0, 1000);
        assert!(g.releases_due(1499).is_empty());
        assert_eq!(g.releases_due(1500), vec![0]);
        assert!(g.releases_due(9999).is_empty());
    }

    #[test]
    fn guard_new_session_drains_held() {
        let mut g = ButtonGuard::default();
        g.note_down(0, 0);
        g.note_down(1, 0);
        assert_eq!(g.new_session(100), vec![0, 1]);
        assert!(g.new_session(200).is_empty());
    }

    #[test]
    fn session_applies_move_on_flush_and_keeps_drag_order() {
        let mut inj = MockInjector::default();
        let mut st = SessionState::new();
        st.on_packet(11, Message::Move { dx: 5, dy: 0 }, 0, &mut inj);
        st.on_packet(11, Message::Move { dx: 3, dy: 0 }, 1, &mut inj);
        assert_eq!(inj.moved, (0, 0));
        st.on_packet(11, Message::ButtonDown { button: 0 }, 2, &mut inj);
        // Flush-before-button: moves land before DOWN.
        assert_eq!(inj.moved, (8, 0));
        assert_eq!(inj.held, 1);
        st.on_packet(11, Message::Move { dx: 1, dy: 1 }, 3, &mut inj);
        st.on_packet(11, Message::ButtonUp { button: 0 }, 4, &mut inj);
        assert_eq!(inj.moved, (9, 1));
        assert_eq!(inj.held, 0);
    }

    #[test]
    fn session_change_releases_held_buttons() {
        let mut inj = MockInjector::default();
        let mut st = SessionState::new();
        st.on_packet(11, Message::ButtonDown { button: 0 }, 0, &mut inj);
        assert_eq!(inj.held, 1);
        // Phone died mid-drag; a new session arrives: stuck button released.
        st.on_packet(99, Message::Move { dx: 1, dy: 0 }, 10, &mut inj);
        assert_eq!(inj.held, 0);
        assert_eq!(inj.ups, 1); // the stuck DOWN got its UP
        st.flush(&mut inj);
        assert_eq!(inj.moved, (1, 0));
    }

    #[test]
    fn silence_timeout_releases_held_buttons() {
        let mut inj = MockInjector::default();
        let mut st = SessionState::new();
        st.on_packet(11, Message::ButtonDown { button: 0 }, 0, &mut inj);
        assert!(st.check_silence(100, &mut inj).is_empty());
        assert_eq!(inj.held, 1);
        assert_eq!(st.check_silence(SILENCE_TIMEOUT_MS, &mut inj), vec![0]);
        assert_eq!(inj.held, 0);
    }

    #[test]
    fn ping_yields_pong_reply() {
        let mut inj = MockInjector::default();
        let mut st = SessionState::new();
        let reply = st
            .on_packet(3, Message::Ping { timestamp_ms: 7 }, 0, &mut inj)
            .expect("PING replies");
        let pkt = trackpad_core::decode(&reply).expect("reply decodes");
        assert_eq!(pkt.msg, Message::Pong { timestamp_ms: 7 });
    }

    fn secure_pair() -> (UdpKeys, [u8; 32]) {
        let keys = UdpKeys {
            phone_key: [11u8; 32],
            desktop_key: [22u8; 32],
        };
        (keys, [7u8; 32])
    }

    #[test]
    fn secure_move_applies_and_replay_drops() {
        use trackpad_core::crypto::seal;
        let (keys, peer) = secure_pair();
        let mut inj = MockInjector::default();
        let mut st = SessionState::new();
        // No channel yet: everything ignored.
        let pkt = seal(&keys.phone_key, 5, 1, Message::Move { dx: 4, dy: 0 });
        assert!(st.on_secure_datagram(&pkt, true, 0, &mut inj).is_none());
        assert_eq!(st.rx_ok, 0);

        st.set_secure(peer, keys.clone(), &mut inj);
        let pkt = seal(&keys.phone_key, 5, 1, Message::Move { dx: 4, dy: 0 });
        assert!(st.on_secure_datagram(&pkt, true, 0, &mut inj).is_none());
        st.flush(&mut inj);
        assert_eq!(inj.moved, (4, 0));
        assert_eq!(st.rx_ok, 1);
        // Replay of the same packet: dropped, no double-apply.
        assert!(st.on_secure_datagram(&pkt, true, 1, &mut inj).is_none());
        st.flush(&mut inj);
        assert_eq!(inj.moved, (4, 0));
        assert_eq!(st.rx_drop, 1);
    }

    #[test]
    fn secure_untrusted_and_tampered_drop_silently() {
        use trackpad_core::crypto::seal;
        let (keys, peer) = secure_pair();
        let mut inj = MockInjector::default();
        let mut st = SessionState::new();
        st.set_secure(peer, keys.clone(), &mut inj);
        // Revoked peer: trusted=false drops before crypto.
        let pkt = seal(
            &keys.phone_key,
            5,
            1,
            Message::Click {
                button: BUTTON_LEFT,
            },
        );
        assert!(st.on_secure_datagram(&pkt, false, 0, &mut inj).is_none());
        assert_eq!(inj.clicks, 0);
        // Tampered: auth fails.
        let mut bad = seal(
            &keys.phone_key,
            5,
            2,
            Message::Click {
                button: BUTTON_LEFT,
            },
        );
        bad[20] ^= 0xFF;
        assert!(st.on_secure_datagram(&bad, true, 0, &mut inj).is_none());
        assert_eq!(inj.clicks, 0);
        assert_eq!(st.rx_drop, 2);
    }

    #[test]
    fn secure_ping_gets_encrypted_pong() {
        use trackpad_core::crypto::{open, seal};
        let (keys, peer) = secure_pair();
        let mut inj = MockInjector::default();
        let mut st = SessionState::new();
        st.set_secure(peer, keys.clone(), &mut inj);
        let ping = seal(&keys.phone_key, 5, 9, Message::Ping { timestamp_ms: 77 });
        let reply = st
            .on_secure_datagram(&ping, true, 0, &mut inj)
            .expect("PONG reply");
        let (_, _, msg) = open(&keys.desktop_key, &reply).expect("opens with desktop key");
        assert_eq!(msg, Message::Pong { timestamp_ms: 77 });
    }

    #[test]
    fn set_and_clear_secure_release_held_buttons() {
        let (keys, peer) = secure_pair();
        let mut inj = MockInjector::default();
        let mut st = SessionState::new();
        st.set_secure(peer, keys.clone(), &mut inj);
        st.on_packet(5, Message::ButtonDown { button: 0 }, 0, &mut inj);
        assert_eq!(inj.held, 1);
        // Rekey (new approval): stuck button released.
        st.set_secure(peer, keys.clone(), &mut inj);
        assert_eq!(inj.held, 0);
        st.on_packet(5, Message::ButtonDown { button: 0 }, 1, &mut inj);
        st.clear_secure(&mut inj);
        assert_eq!(inj.held, 0);
        assert!(st.secure_peer().is_none());
    }

    #[test]
    fn applies_move_and_click() {
        let mut inj = MockInjector::default();
        let mut session = 0;
        let mv = trackpad_core::encode(11, 1, Message::Move { dx: 5, dy: -3 });
        assert_eq!(
            handle_datagram(&mv, &mut session, &mut inj),
            Ok(Outcome::Applied)
        );
        let cl = trackpad_core::encode(
            11,
            2,
            Message::Click {
                button: BUTTON_LEFT,
            },
        );
        assert_eq!(
            handle_datagram(&cl, &mut session, &mut inj),
            Ok(Outcome::Applied)
        );
        assert_eq!(inj.moved, (5, -3));
        assert_eq!(inj.clicks, 1);
        assert_eq!(session, 11);
    }

    #[test]
    fn ping_yields_pong_reply_legacy() {
        let mut inj = MockInjector::default();
        let mut session = 0;
        let ping = trackpad_core::encode(
            3,
            8,
            Message::Ping {
                timestamp_ms: 12345,
            },
        );
        let out = handle_datagram(&ping, &mut session, &mut inj).expect("valid ping");
        match out {
            Outcome::Reply(bytes) => {
                let pkt = trackpad_core::decode(&bytes).expect("reply decodes");
                assert_eq!(
                    pkt.msg,
                    Message::Pong {
                        timestamp_ms: 12345
                    }
                );
                assert_eq!(pkt.session_id, 3);
            }
            Outcome::Applied => panic!("PING must produce a reply"),
        }
        assert_eq!(inj.moved, (0, 0));
        assert_eq!(inj.clicks, 0);
    }

    #[test]
    fn malformed_packet_errors_without_touching_injector() {
        let mut inj = MockInjector::default();
        let mut session = 0;
        assert!(handle_datagram(&[1, 2, 3], &mut session, &mut inj).is_err());
        assert_eq!(inj.moved, (0, 0));
        assert_eq!(inj.clicks, 0);
    }
}
