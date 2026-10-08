//! `trackpad-service`: secure desktop receiver (Phase 4).
//!
//! - TCP pairing/reconnect server on 51516 (or `--tcp-port`), Noise
//!   handshakes, explicit UI approval for new devices.
//! - Encrypted UDP input on 51515 with replay protection. Plaintext (v1)
//!   packets are dropped unless `--open` is passed (LAN testing only).
//! - mDNS advertisement `_wptrackpad._tcp` unless `--no-mdns`.
//! - Interactive console: `pair` (new QR ceremony), `approve <id>`,
//!   `deny <id>`, `list`, `revoke <hex-pubkey>`, `status`, `quit`.
//!   `--auto-approve` approves the first pending device WITHOUT asking
//!   (testing only — never use on an untrusted network).
//!
//! Windows Firewall will prompt on first run: allow on private networks.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use trackpad_core::{DEFAULT_TCP_PORT, DEFAULT_UDP_PORT};
use trackpad_desktop::pairing_server::{serve, Ceremony, PairingShared};
use trackpad_desktop::service::{default_bind, from_hex, hex, udp_loop};

fn usage() -> ! {
    eprintln!("usage: trackpad-service [--bind ADDR:PORT] [--tcp-port P] [--open] [--pair] [--webpin] [--auto-approve] [--no-mdns]");
    std::process::exit(2);
}

fn main() {
    let mut bind = default_bind();
    let mut tcp_port = DEFAULT_TCP_PORT;
    let mut open = false;
    let mut pair_at_start = false;
    let mut webpin_at_start = false;
    let mut auto_approve = false;
    let mut mdns = true;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--bind" => bind = args.next().unwrap_or_else(|| usage()),
            "--tcp-port" => {
                tcp_port = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--open" => open = true,
            "--pair" => pair_at_start = true,
            "--webpin" => webpin_at_start = true,
            "--auto-approve" => auto_approve = true,
            "--no-mdns" => mdns = false,
            _ => usage(),
        }
    }
    if auto_approve {
        eprintln!("WARNING: --auto-approve trusts the next pairing device with NO confirmation.");
    }
    if open {
        eprintln!("WARNING: --open accepts unauthenticated plaintext input. LAN testing only.");
    }

    let identity = trackpad_desktop::trust::DeviceIdentity::load_or_create().unwrap_or_else(|e| {
        eprintln!("trackpad-service: identity failed: {e}");
        std::process::exit(1);
    });
    let trust = trackpad_desktop::trust::TrustStore::load().unwrap_or_else(|e| {
        eprintln!("trackpad-service: trust store failed: {e}");
        std::process::exit(1);
    });
    let udp_port: u16 = bind
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_UDP_PORT);
    let fp: [u8; 32] = identity
        .public_key
        .as_slice()
        .try_into()
        .expect("stored public key is 32 bytes");
    let shared = Arc::new(Mutex::new(PairingShared::new(
        identity.private_key,
        identity.public_key,
        identity.device_name.clone(),
        udp_port,
        trust,
    )));

    let listener = TcpListener::bind(format!("0.0.0.0:{tcp_port}")).unwrap_or_else(|e| {
        eprintln!("trackpad-service: cannot bind TCP {tcp_port}: {e}");
        std::process::exit(1);
    });
    let tcp_shared = Arc::clone(&shared);
    std::thread::spawn(move || serve(listener, tcp_shared));

    if mdns {
        match trackpad_desktop::discovery::Advertiser::start(
            &identity.device_name,
            &fp,
            tcp_port,
            udp_port,
        ) {
            Ok(a) => {
                println!("trackpad-service: advertising {}", a.fullname());
                std::mem::forget(a); // live for the process lifetime
            }
            Err(e) => eprintln!("trackpad-service: mDNS failed (continuing): {e}"),
        }
    }

    if pair_at_start {
        print_ceremony(&shared);
    }
    if auto_approve {
        let s = Arc::clone(&shared);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let id = s.lock().expect("lock").pending().first().map(|p| p.id);
            if let Some(id) = id {
                s.lock().expect("lock").approve(id, true);
                eprintln!("trackpad-service: auto-approved pending device {id}");
            }
        });
    }

    // UDP loop runs on a thread; the main thread serves the console REPL.
    let udp_shared = Arc::clone(&shared);
    let udp_handle = std::thread::spawn(move || udp_loop(&bind, udp_shared, open));
    // Browser trackpad (no install): PIN-gated page on WEB_PORT.
    let web = Arc::new(Mutex::new(trackpad_desktop::web::WebState::new()));
    if let Ok(listener) =
        std::net::TcpListener::bind(format!("0.0.0.0:{}", trackpad_desktop::web::WEB_PORT))
    {
        let web_shared = Arc::clone(&web);
        std::thread::spawn(move || trackpad_desktop::web::serve(listener, web_shared));
        println!(
            "trackpad-service: browser trackpad at http://<this-pc>:{} (type 'webpin' for a code)",
            trackpad_desktop::web::WEB_PORT
        );
    }
    if webpin_at_start {
        match web.lock().expect("lock").begin() {
            Ok(code) => println!("browser code: {code} (10 min)"),
            Err(e) => eprintln!("webpin failed: {e}"),
        }
    }
    repl(shared, web);
    // REPL quit returns: process exits (UDP thread dies with it).
    let _ = udp_handle;
}

/// Start everything the binary starts, but for embedding (Tauri).
/// Thin wrapper over [`trackpad_desktop::service::boot`].
pub fn boot_embedded(udp_port: u16, tcp_port: u16, mdns: bool) -> Arc<Mutex<PairingShared>> {
    trackpad_desktop::service::boot(udp_port, tcp_port, mdns).shared
}

fn print_ceremony(shared: &Arc<Mutex<PairingShared>>) {
    match shared.lock().expect("lock").begin_ceremony() {
        Ok(Ceremony {
            qr_text,
            qr_svg,
            short_code,
            expires_at_ms,
        }) => {
            println!("--- pairing ceremony (expires in ~2 min) ---");
            println!("QR: {qr_text}");
            println!("short code: {short_code} (expires {expires_at_ms} ms)");
            println!(
                "QR SVG: {} bytes (rendered in the desktop UI)",
                qr_svg.len()
            );
            println!("On the phone: scan the QR (or enter the code), then approve here.");
        }
        Err(e) => eprintln!("trackpad-service: ceremony failed: {e}"),
    }
}

fn repl(shared: Arc<Mutex<PairingShared>>, web: Arc<Mutex<trackpad_desktop::web::WebState>>) {
    println!("trackpad-service: console ready (pair | webpin | approve <id> | deny <id> | list | revoke <hex> | status | quit)");
    let stdin = std::io::stdin();
    loop {
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => {
                // No console attached (background run): stay alive.
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
            Ok(_) => {}
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
        }
        let mut parts = line.split_whitespace();
        match parts.next().unwrap_or("") {
            "pair" => print_ceremony(&shared),
            "webpin" => match web.lock().expect("lock").begin() {
                Ok(code) => println!("browser code: {code} (10 min, old sessions revoked)"),
                Err(e) => println!("webpin failed: {e}"),
            },
            "approve" => match parts.next().and_then(|s| s.parse::<u64>().ok()) {
                Some(id) => println!(
                    "approve {id}: {}",
                    shared.lock().expect("lock").approve(id, true)
                ),
                None => println!("usage: approve <id>"),
            },
            "deny" => match parts.next().and_then(|s| s.parse::<u64>().ok()) {
                Some(id) => println!(
                    "deny {id}: {}",
                    shared.lock().expect("lock").approve(id, false)
                ),
                None => println!("usage: deny <id>"),
            },
            "list" => {
                let s = shared.lock().expect("lock");
                for p in s.pending() {
                    println!(
                        "pending {} '{}' key={}",
                        p.id,
                        p.device_name,
                        hex(&p.peer_pubkey)
                    );
                }
                for p in s.trust_peers() {
                    println!("trusted '{}' key={}", p.name, hex(&p.pubkey));
                }
                match s.live() {
                    Some(l) => println!("live: '{}' key={}", l.peer_name, hex(&l.peer_pubkey)),
                    None => println!("live: none"),
                }
            }
            "revoke" => match parts.next().map(from_hex) {
                Some(Ok(key)) => println!("revoked: {}", shared.lock().expect("lock").revoke(&key)),
                _ => println!("usage: revoke <64-hex-chars>"),
            },
            "status" => {
                let s = shared.lock().expect("lock");
                println!(
                    "pairing active: {} | pending: {} | trusted: {} | live: {}",
                    s.ceremony_active(),
                    s.pending().len(),
                    s.trust_peers().len(),
                    s.live().is_some()
                );
            }
            "quit" => {
                println!("trackpad-service: bye");
                std::process::exit(0);
            }
            "" => {}
            c => println!("unknown command: {c}"),
        }
    }
}
