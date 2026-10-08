//! `fake-phone`: scripted input sender for testing without a real phone.
//!
//! Plain subcommands (`--open` service only): `move` (default), `rightclick`,
//! `scroll`, `drag`, `stuck` (see Phase 2 docs).
//!
//! Secure subcommands (default service mode):
//! - `pair "<qr-text>"`: runs the full QR ceremony (blocks until the
//!   desktop approves — use `--auto-approve` on the service for tests),
//!   then sends 120 encrypted MOVE + 1 encrypted CLICK.
//! - `ping`: encrypted PING/PONG round trip against the live session keys
//!   is covered by `latency-harness`; see its `--secure` notes.
//!
//! ```powershell
//! cargo run -p fake-phone -- pair "192.168.1.10#WPT1..."
//! ```

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use trackpad_core::{button, encode, Message, BUTTON_LEFT, DEFAULT_UDP_PORT};

fn usage() -> ! {
    eprintln!("usage: fake-phone [TARGET] [--port P] [move|rightclick|scroll|drag|stuck]");
    eprintln!("   or: fake-phone pair \"<qr-text>\" [--tcp-host H]");
    eprintln!("   or: fake-phone pair --qr-file PATH [--tcp-host H]");
    std::process::exit(2);
}

fn main() {
    let mut target = String::from("127.0.0.1");
    let mut port: u16 = DEFAULT_UDP_PORT;
    let mut cmd = String::from("move");
    let mut tcp_host: Option<String> = None;
    let mut positionals: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => {
                port = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--tcp-host" => tcp_host = Some(args.next().unwrap_or_else(|| usage())),
            "--qr-file" => {
                let path = args.next().unwrap_or_else(|| usage());
                positionals.push(
                    std::fs::read_to_string(&path)
                        .unwrap_or_else(|_| usage())
                        .trim()
                        .to_string(),
                );
            }
            s if s.starts_with('-') => usage(),
            s => positionals.push(s.to_string()),
        }
    }
    // `pair "<qr>"` (or lone text containing '#', e.g. via --qr-file)
    // takes the ceremony text.
    if positionals.first().is_some_and(|p| p == "pair") {
        let qr = positionals.get(1).unwrap_or_else(|| usage()).clone();
        pair_and_drive(qr, tcp_host);
    } else if positionals.len() == 1 && positionals[0].contains('#') {
        let qr = positionals[0].clone();
        pair_and_drive(qr, tcp_host);
    }
    match positionals.as_slice() {
        [] => {}
        [one] => {
            if looks_like_host(one) {
                target = one.clone();
            } else {
                cmd = one.clone();
            }
        }
        [t, c, ..] => {
            target = t.clone();
            cmd = c.clone();
        }
    }
    let dest: SocketAddr = format!("{target}:{port}")
        .parse()
        .unwrap_or_else(|_| usage());
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap_or_else(|e| {
        eprintln!("fake-phone: cannot bind: {e}");
        std::process::exit(1);
    });
    let session_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234);

    let mut seq = 0u64;
    let mut send = |msg: Message| {
        let pkt = encode(session_id, seq, msg);
        seq += 1;
        if let Err(e) = socket.send_to(&pkt, dest) {
            eprintln!("fake-phone: send #{seq} failed: {e}");
            std::process::exit(1);
        }
    };
    let tick_120hz = Duration::from_secs_f64(1.0 / 120.0);
    let tick_60hz = Duration::from_secs_f64(1.0 / 60.0);

    match cmd.as_str() {
        "move" => {
            for _ in 0..120 {
                send(Message::Move { dx: 5, dy: 0 });
                std::thread::sleep(tick_120hz);
            }
            send(Message::Click {
                button: BUTTON_LEFT,
            });
            println!("fake-phone: sent 120 MOVE + 1 CLICK to {dest}");
        }
        "rightclick" => {
            send(Message::Click {
                button: button::RIGHT,
            });
            println!("fake-phone: sent right CLICK to {dest}");
        }
        "scroll" => {
            for _ in 0..20 {
                send(Message::Scroll { dx: 0, dy: 40 });
                std::thread::sleep(tick_60hz);
            }
            for _ in 0..20 {
                send(Message::Scroll { dx: 0, dy: -40 });
                std::thread::sleep(tick_60hz);
            }
            println!("fake-phone: sent 20 SCROLL down + 20 up to {dest}");
        }
        "drag" => {
            send(Message::ButtonDown {
                button: BUTTON_LEFT,
            });
            for _ in 0..60 {
                send(Message::Move { dx: 5, dy: 0 });
                std::thread::sleep(tick_120hz);
            }
            // Same id style as a redundant repeat: send UP twice.
            send(Message::ButtonUp {
                button: BUTTON_LEFT,
            });
            send(Message::ButtonUp {
                button: BUTTON_LEFT,
            });
            println!("fake-phone: sent drag (DOWN + 60 MOVE + UP x2) to {dest}");
        }
        "stuck" => {
            send(Message::ButtonDown {
                button: BUTTON_LEFT,
            });
            println!("fake-phone: sent DOWN with no UP; watch the service release it");
        }
        _ => usage(),
    }
}

/// Full secure loop: QR ceremony, then encrypted MOVE + CLICK.
/// Blocks until the desktop approves (run the service with --auto-approve
/// for unattended tests).
fn pair_and_drive(qr_text: String, tcp_host: Option<String>) -> ! {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let session = trackpad_desktop::pairing_client::pair_qr(
        &qr_text,
        tcp_host.as_deref(),
        "fake-phone",
        now_unix,
    )
    .unwrap_or_else(|e| {
        eprintln!("fake-phone: pairing failed: {e}");
        std::process::exit(1);
    });
    let host = qr_text
        .split_once('#')
        .map(|(h, _)| h.to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let dest: SocketAddr = format!("{}:{}", host, session.udp_port)
        .parse()
        .unwrap_or_else(|_| usage());
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap_or_else(|e| {
        eprintln!("fake-phone: cannot bind: {e}");
        std::process::exit(1);
    });
    let sid: u64 = now_unix ^ 0x1234_5678;
    let tick = Duration::from_secs_f64(1.0 / 120.0);
    for i in 0..120u64 {
        let pkt = trackpad_core::crypto::seal(
            &session.keys.phone_key,
            sid,
            i,
            Message::Move { dx: 5, dy: 0 },
        );
        if let Err(e) = socket.send_to(&pkt, dest) {
            eprintln!("fake-phone: MOVE #{i} failed: {e}");
            std::process::exit(1);
        }
        std::thread::sleep(tick);
    }
    let click = trackpad_core::crypto::seal(
        &session.keys.phone_key,
        sid,
        120,
        Message::Click {
            button: BUTTON_LEFT,
        },
    );
    if let Err(e) = socket.send_to(&click, dest) {
        eprintln!("fake-phone: CLICK failed: {e}");
        std::process::exit(1);
    }
    println!("fake-phone: paired + sent 120 encrypted MOVE + 1 CLICK to {dest}");
    std::process::exit(0);
}

/// Command words vs hosts: a lone positional naming a command selects the
/// demo; anything else is the target address.
fn looks_like_host(s: &str) -> bool {
    !matches!(s, "move" | "rightclick" | "scroll" | "drag" | "stuck")
}
