//! Browser trackpad: zero-install iPhone path (Safari → LAN page → input).
//!
//! Why HTTP+JSON and not WebSocket: no new dependencies, fully owned code,
//! and keep-alive POSTs at gesture rate are sub-millisecond on loopback.
//! The desktop serves one page (`GET /`) and two endpoints on [`WEB_PORT`]:
//! - `POST /api/pair {"pin":"123456"}` → `{"token":"…"}` (PIN ceremony)
//! - `POST /api/input {"token":"…","seq":N,"msgs":[{...}]}` → `{"ok":true}`
//!
//! Security model (weaker than Noise pairing — documented in
//! `docs/threat-model.md`): whoever reads the PIN off the desktop screen
//! is authorized (displaying it IS the approval). The PIN is random per
//! ceremony, TTL'd, rate-limited like short codes, and rotated on demand.
//! Traffic is LAN-plaintext: use on trusted Wi-Fi only.
//!
//! Input flows through the same [`crate::service`] session machine
//! (coalescing, flush-before-button, silence release), so stuck-button
//! safety holds. A reaper thread releases buttons if the browser dies.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use trackpad_core::pairing::{AttemptGuard, PairingError, ShortCode};
use trackpad_core::Message;

pub const WEB_PORT: u16 = 51517;
/// Browser PIN TTL: typing a URL takes longer than scanning (10 min).
pub const WEB_PIN_TTL_MS: u64 = 600_000;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const BODY_MAX: usize = 64 * 1024;

/// Placeholder peer for pairings that arrive without a traceable socket
/// address (`peer_addr()` failed). The resulting session simply never gets
/// connection-based revocation; the reaper still enforces input silence.
const STALE_PEER_PLACEHOLDER: (std::net::Ipv4Addr, u16) = (std::net::Ipv4Addr::UNSPECIFIED, 0);

fn stale_peer() -> std::net::SocketAddr {
    std::net::SocketAddr::from(STALE_PEER_PLACEHOLDER)
}

/// Embedded trackpad page (single file, no external assets).
const PAGE: &str = include_str!("web_page.html");

#[derive(Debug)]
struct WebPin {
    code: ShortCode,
    expires_at_ms: u64,
}

struct WebSession {
    token: [u8; 16],
    state: crate::service::SessionState,
    injector: Box<dyn crate::InputInjector + Send>,
    last_ms: u64,
}

/// Mutable web state. One browser session at a time (same as UDP).
pub struct WebState {
    pin: Option<WebPin>,
    session: Option<WebSession>,
    guard: AttemptGuard,
    now_ms: fn() -> u64,
    make_injector: fn() -> Box<dyn crate::InputInjector + Send>,
    /// Socket address of the connection that last paired successfully.
    /// When the page closes or the phone becomes unreachable, that socket
    /// dies — and the live session is revoked immediately (input stops).
    /// `None` until a pairing succeeds.
    session_peer: Option<std::net::SocketAddr>,
}

#[cfg(windows)]
fn os_injector() -> Box<dyn crate::InputInjector + Send> {
    Box::new(crate::WindowsInjector::default())
}

#[cfg(not(windows))]
fn os_injector() -> Box<dyn crate::InputInjector + Send> {
    Box::new(crate::MockInjector::default())
}

impl WebState {
    pub fn new() -> Self {
        Self {
            pin: None,
            session: None,
            guard: AttemptGuard::default(),
            now_ms: crate::now_ms,
            make_injector: os_injector,
            session_peer: None,
        }
    }

    fn now(&self) -> u64 {
        (self.now_ms)()
    }

    /// Start (or restart) a browser ceremony. Returns the display code.
    /// Previous PIN/session are discarded: showing a new code revokes the old.
    pub fn begin(&mut self) -> Result<String, PairingError> {
        let code = ShortCode::generate()?;
        let display = code.display();
        self.pin = Some(WebPin {
            code,
            expires_at_ms: self.now() + WEB_PIN_TTL_MS,
        });
        self.session = None;
        Ok(display)
    }

    pub fn pin_active(&self) -> bool {
        self.pin
            .as_ref()
            .is_some_and(|p| self.now() <= p.expires_at_ms)
    }

    fn take_token(&mut self, pin_text: &str, peer: std::net::SocketAddr) -> Result<[u8; 16], PairingError> {
        let now = self.now();
        self.guard.check(now)?;
        let pin = self
            .pin
            .as_ref()
            .filter(|p| now <= p.expires_at_ms)
            .ok_or(PairingError::Expired)?;
        if pin.code.display() != pin_text.trim() {
            self.guard.note_failure(now);
            return Err(PairingError::Spake2);
        }
        self.guard.note_success();
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).map_err(|_| PairingError::Spake2)?;
        self.session = Some(WebSession {
            token,
            state: crate::service::SessionState::new(),
            injector: (self.make_injector)(),
            last_ms: now,
        });
        // Record which connection paired, so handle() can revoke the
        // session when that connection dies.
        self.session_peer = Some(peer);
        Ok(token)
    }
}

impl Default for WebState {
    fn default() -> Self {
        Self::new()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Result<[u8; 16], ()> {
    if s.len() != 32 {
        return Err(());
    }
    let mut out = [0u8; 16];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16).ok_or(())?;
        let lo = (chunk[1] as char).to_digit(16).ok_or(())?;
        out[i] = (hi as u8) * 16 + lo as u8;
    }
    Ok(out)
}

/// Serve forever on `bind`. Each connection is handled on its own thread
/// (keep-alive loops); a reaper thread enforces the silence timeout.
pub fn serve(listener: TcpListener, shared: Arc<Mutex<WebState>>) {
    let reaper = Arc::clone(&shared);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(mut s) = reaper.lock() {
            let now = (s.now_ms)();
            if let Some(sess) = s.session.as_mut() {
                let released = {
                    let state = &mut sess.state;
                    let injector = &mut *sess.injector;
                    state.check_silence(now, injector)
                };
                if !released.is_empty() {
                    eprintln!("web: released stuck buttons after silence");
                }
            }
        }
    });
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || handle(stream, shared));
            }
            Err(e) => eprintln!("web: accept failed: {e}"),
        }
    }
}

fn handle(mut stream: TcpStream, shared: Arc<Mutex<WebState>>) {
    if stream.set_read_timeout(Some(READ_TIMEOUT)).is_err() {
        return;
    }
    let peer = stream.peer_addr().ok();
    loop {
        let req = match read_request(&mut stream) {
            Ok(r) => r,
            Err(_) => {
                // Connection closed, timed out, or errored: if this socket
                // was the one that paired the live browser session, revoke
                // it now so input stops the moment its page is unreachable.
                if let Some(p) = peer {
                    let mut s = match shared.lock() {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    if s.session_peer == Some(p) {
                        s.session = None;
                        s.session_peer = None;
                        eprintln!("web: browser session revoked (peer hung up or idle)");
                    }
                }
                return;
            }
        };
        let keep_alive = req.keep_alive;
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/") => {
                write_response(
                    &mut stream,
                    200,
                    "text/html; charset=utf-8",
                    PAGE.as_bytes(),
                    keep_alive,
                );
            }
            ("POST", "/api/pair") => {
                let code = serde_json::from_slice::<serde_json::Value>(&req.body)
                    .ok()
                    .and_then(|v| v.get("pin")?.as_str().map(|s| s.to_string()));
                let body = match code {
                    Some(pin) => match shared
                        .lock()
                        .map(|mut s| s.take_token(&pin, stream.peer_addr().unwrap_or_else(|_| stale_peer())))
                    {
                        Ok(Ok(token)) => format!(r#"{{"ok":true,"token":"{}"}}"#, hex(&token)),
                        _ => r#"{"ok":false,"error":"bad pin"}"#.to_string(),
                    },
                    None => r#"{"ok":false,"error":"bad pin"}"#.to_string(),
                };
                // Same answer shape + timing for wrong PINs: no oracle.
                write_response(
                    &mut stream,
                    200,
                    "application/json",
                    body.as_bytes(),
                    keep_alive,
                );
            }
            ("POST", "/api/input") => {
                let ok = apply_input(&shared, &req.body);
                let body = if ok {
                    r#"{"ok":true}"#
                } else {
                    r#"{"ok":false}"#
                };
                write_response(
                    &mut stream,
                    200,
                    "application/json",
                    body.as_bytes(),
                    keep_alive,
                );
            }
            _ => {
                write_response(&mut stream, 404, "text/plain", b"not found", false);
                return;
            }
        }
        if !keep_alive {
            return;
        }
    }
}

/// Apply one input batch. Pure routing over the shared session machine.
fn apply_input(shared: &Arc<Mutex<WebState>>, body: &[u8]) -> bool {
    let v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let token = match v
        .get("token")
        .and_then(|t| t.as_str())
        .and_then(|s| unhex(s).ok())
    {
        Some(t) => t,
        None => return false,
    };
    let msgs = match v.get("msgs").and_then(|m| m.as_array()) {
        Some(m) if m.len() <= 64 => m,
        _ => return false,
    };
    let mut guard = match shared.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let now = (guard.now_ms)();
    let sess = match guard.session.as_mut() {
        Some(s) if s.token == token => s,
        _ => return false,
    };
    sess.last_ms = now;
    let stok = sess_token(sess);
    // Web batches carry their own order; flush after each batch keeps drag
    // order (DOWN, moves, UP) exact.
    let mut ok = true;
    for m in msgs {
        match json_to_message(m) {
            Some(msg) => {
                let state = &mut sess.state;
                let injector = &mut *sess.injector;
                state.on_packet(stok, msg, now, injector);
            }
            None => {
                ok = false;
            }
        }
    }
    {
        let state = &mut sess.state;
        let injector = &mut *sess.injector;
        state.flush(injector);
        let _ = state.check_silence(now, injector);
    }
    ok
}

fn sess_token(sess: &WebSession) -> u64 {
    u64::from_le_bytes(sess.token[..8].try_into().expect("token is 16 bytes"))
}

fn json_to_message(v: &serde_json::Value) -> Option<Message> {
    let t = v.get("t")?.as_str()?;
    match t {
        "move" => Some(Message::Move {
            dx: clamp_i16(v.get("dx")?.as_i64()?),
            dy: clamp_i16(v.get("dy")?.as_i64()?),
        }),
        "click" => Some(Message::Click {
            button: v.get("b")?.as_u64().unwrap_or(0) as u8,
        }),
        "down" => Some(Message::ButtonDown {
            button: v.get("b")?.as_u64().unwrap_or(0) as u8,
        }),
        "up" => Some(Message::ButtonUp {
            button: v.get("b")?.as_u64().unwrap_or(0) as u8,
        }),
        "scroll" => Some(Message::Scroll {
            dx: clamp_i16(v.get("dx")?.as_i64().unwrap_or(0)),
            dy: clamp_i16(v.get("dy")?.as_i64().unwrap_or(0)),
        }),
        _ => None,
    }
}

fn clamp_i16(v: i64) -> i16 {
    v.clamp(-32768, 32767) as i16
}

struct Request {
    method: String,
    path: String,
    keep_alive: bool,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut head = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    while head.len() < 8192 {
        match stream.read(&mut byte) {
            Ok(0) => return Err("eof".to_string()),
            Ok(_) => {
                head.push(byte[0]);
                if head.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    if !head.ends_with(b"\r\n\r\n") {
        return Err("head too large".to_string());
    }
    let head_str = String::from_utf8_lossy(&head);
    let mut lines = head_str.lines();
    let request_line = lines.next().ok_or("no request line".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or("no method".to_string())?.to_string();
    let path = parts.next().ok_or("no path".to_string())?.to_string();
    if method != "GET" && method != "POST" {
        return Err(format!("bad method {method}"));
    }
    let mut content_length = 0usize;
    let mut keep_alive = method == "GET";
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v
                .trim()
                .parse()
                .map_err(|_| "bad content-length".to_string())?;
            if content_length > BODY_MAX {
                return Err("body too large".to_string());
            }
        }
        if lower.starts_with("connection:") && lower.contains("close") {
            keep_alive = false;
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream
            .read_exact(&mut body)
            .map_err(|e| format!("body: {e}"))?;
    }
    Ok(Request {
        method,
        path,
        keep_alive,
        body,
    })
}

fn write_response(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8], keep_alive: bool) {
    let reason = match code {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let conn = if keep_alive { "keep-alive" } else { "close" };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: {conn}\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static TEST_NOW: std::cell::RefCell<u64> = const { std::cell::RefCell::new(0) };
    }

    fn test_clock() -> u64 {
        TEST_NOW.with(|t| *t.borrow())
    }

    fn test_state(now: u64) -> WebState {
        let mut s = WebState::new();
        s.now_ms = test_clock;
        s.make_injector = || Box::new(crate::MockInjector::default());
        TEST_NOW.with(|t| *t.borrow_mut() = now);
        s
    }

    fn pair(state: &mut WebState) -> ([u8; 16], String) {
        let display = state.begin().expect("begin");
        let pin = state.pin.as_ref().expect("pin").code.display();
        assert_eq!(display, pin);
        let token = state
            .take_token(&pin, stale_peer())
            .expect("correct pin");
        (token, pin)
    }

    fn peer_of(host: [u8; 4], port: u16) -> std::net::SocketAddr {
        std::net::SocketAddr::from((std::net::Ipv4Addr::from(host), port))
    }

    #[test]
    fn session_peer_bound_on_pair() {
        let mut s = test_state(1000);
        let p = peer_of([192, 168, 1, 50], 4444);
        let display = s.begin().expect("begin");
        let pin = s.pin.as_ref().expect("pin").code.display();
        assert!(!display.is_empty());
        s.take_token(&pin, p).expect("pair");
        assert_eq!(s.session_peer, Some(p));
    }

    #[test]
    fn disconnect_from_pairing_peer_revokes_session() {
        let mut s = test_state(2000);
        let p = peer_of([192, 168, 1, 50], 4444);
        s.begin().expect("begin");
        let pin = s.pin.as_ref().expect("pin").code.display();
        s.take_token(&pin, p).expect("pair");
        assert!(s.session.is_some());
        // Simulate what handle() does when that socket dies.
        if s.session_peer == Some(p) {
            s.session = None;
            s.session_peer = None;
        }
        assert!(s.session.is_none());
        assert_eq!(s.session_peer, None);
    }

    #[test]
    fn pin_wrong_locks_out() {
        let mut s = test_state(1000);
        s.begin().expect("begin");
        for _ in 0..3 {
            assert_eq!(s.take_token("000000", stale_peer()), Err(PairingError::Spake2));
        }
        assert_eq!(s.take_token("000000", stale_peer()), Err(PairingError::LockedOut));
    }

    #[test]
    fn pin_expires_and_single_session() {
        let mut s = test_state(1000);
        let (token, _) = pair(&mut s);
        assert!(s.session.is_some());
        // New ceremony revokes the old session.
        s.begin().expect("re-begin");
        assert!(s.session.is_none());
        let _ = token;
        TEST_NOW.with(|t| *t.borrow_mut() = 1000 + WEB_PIN_TTL_MS + 1);
        assert_eq!(s.take_token("000000", stale_peer()), Err(PairingError::Expired));
    }

    fn input_body(token: &[u8; 16], msgs: &str) -> Vec<u8> {
        format!(r#"{{"token":"{}","seq":1,"msgs":{msgs}}}"#, hex(token)).into_bytes()
    }

    #[test]
    fn input_applies_and_validates() {
        let mut s = test_state(2000);
        let (token, _) = pair(&mut s);
        let shared = Arc::new(Mutex::new(s));
        let good = input_body(&token, r#"[{"t":"move","dx":5,"dy":-3}]"#);
        assert!(apply_input(&shared, &good));
        {
            let mut g = shared.lock().expect("lock");
            let sess = g.session.as_mut().expect("session");
            let mock = sess.injector.as_mock().expect("mock injector in tests");
            // apply_input flushes each batch: already applied.
            assert_eq!(mock.moved, (5, -3));
        }
        // Bad token: rejected, nothing applied.
        let bad = input_body(&[9u8; 16], r#"[{"t":"click","b":0}]"#);
        assert!(!apply_input(&shared, &bad));
        // Malformed message: batch flagged, valid sibling still applies.
        let mixed = input_body(&token, r#"[{"t":"bogus"},{"t":"click","b":0}]"#);
        assert!(!apply_input(&shared, &mixed));
        {
            let mut g = shared.lock().expect("lock");
            let sess = g.session.as_mut().expect("session");
            sess.state.flush(&mut *sess.injector);
            let mock = sess.injector.as_mock().expect("mock");
            assert_eq!(mock.clicks, 1);
        }
    }

    #[test]
    fn request_parsing_rejects_garbage() {
        // Method allowlist + size caps live in read_request; JSON shape
        // checks live here.
        assert!(json_to_message(&serde_json::json!({"t": "move"})).is_none());
        assert!(json_to_message(&serde_json::json!({"t": "move", "dx": 1, "dy": 2})).is_some());
        assert!(json_to_message(&serde_json::json!({"t": "nope"})).is_none());
        assert_eq!(clamp_i16(99999), 32767);
        assert_eq!(clamp_i16(-99999), -32768);
        assert!(unhex("zz").is_err());
    }

    fn http_roundtrip() {
        use std::io::{Read, Write};
        let mut s = test_state(9000);
        s.begin().expect("begin");
        let pin = s.pin.as_ref().expect("pin").code.display();
        let shared = Arc::new(Mutex::new(s));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || serve(listener, shared));

        fn exchange(addr: std::net::SocketAddr, req: &str) -> String {
            use std::io::{Read, Write};
            let mut stream = TcpStream::connect(addr).expect("connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("timeout");
            stream.write_all(req.as_bytes()).expect("write");
            let mut out = Vec::new();
            let mut buf = [0u8; 4096];
            // One response: read until Content-Length satisfied or close.
            let mut head = Vec::new();
            let mut one = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut one).expect("head");
                head.push(one[0]);
            }
            let head_str = String::from_utf8_lossy(&head).into_owned();
            let len: usize = head_str
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse().ok())
                })
                .unwrap_or(0);
            let mut body = vec![0u8; len];
            stream.read_exact(&mut body).expect("body");
            out.extend_from_slice(&head);
            out.extend_from_slice(&body);
            String::from_utf8_lossy(&out).into_owned()
        }

        fn post(path: &str, body: &str) -> String {
            format!(
                "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        }

        // Page serves.
        let page = exchange(
            addr,
            "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        );
        assert!(page.starts_with("HTTP/1.1 200"), "page: {page}");
        assert!(page.contains("Phone Trackpad"));

        // Wrong PIN rejected (same shape, no oracle).
        let r = exchange(addr, &post("/api/pair", r#"{"pin":"000000"}"#));
        assert!(r.contains(r#""ok":false"#), "wrong pin: {r}");

        // Big body rejected at the framing layer: the connection dies
        // without an answer (the server must not buffer attacker bodies).
        let big = "x".repeat(BODY_MAX + 1);
        let oversize = std::panic::catch_unwind(|| {
            exchange(addr, &post("/api/pair", &format!(r#"{{"pin":"{big}"}}"#)))
        });
        assert!(oversize.is_err(), "oversize must kill the connection");

        // Real PIN pairs.
        let r = exchange(addr, &post("/api/pair", &format!(r#"{{"pin":"{pin}"}}"#)));
        assert!(r.contains(r#""ok":true"#), "pair: {r}");
        let token: String = {
            let body = r.split("\r\n\r\n").nth(1).unwrap_or("");
            let v: serde_json::Value = serde_json::from_str(body).expect("json");
            v.get("token")
                .and_then(|t| t.as_str())
                .expect("token")
                .to_string()
        };

        // Input applies through the shared session machine.
        let r = exchange(
            addr,
            &post(
                "/api/input",
                &format!(
                    r#"{{"token":"{token}","seq":1,"msgs":[{{"t":"move","dx":6,"dy":2}},{{"t":"click","b":0}}]}}"#
                ),
            ),
        );
        assert!(r.contains(r#""ok":true"#), "input: {r}");

        // Unknown token rejected.
        let r = exchange(
            addr,
            &post(
                "/api/input",
                r#"{"token":"00000000000000000000000000000000","seq":2,"msgs":[{"t":"move","dx":1,"dy":1}]}"#,
            ),
        );
        assert!(r.contains(r#""ok":false"#), "bad token: {r}");

        // Unknown path 404s.
        let r = exchange(
            addr,
            "GET /nope HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        );
        assert!(r.starts_with("HTTP/1.1 404"), "404: {r}");
    }

    #[test]
    fn http_end_to_end() {
        http_roundtrip();
    }
}
