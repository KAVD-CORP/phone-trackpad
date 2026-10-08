//! Phase 4 latency harness: PING/PONG round-trip statistics.
//!
//! Plaintext mode (service `--open`, LAN testing only):
//! `latency-harness 127.0.0.1 --count 200 --rate 60`
//!
//! Encrypted mode: pass the ceremony QR text (service `--pair
//! --auto-approve` for unattended runs). The harness pairs, then sends v2
//! sealed PINGs and reports the same stats. RTT/2 approximates one-way
//! phone→desktop latency on the same path.
//!
//! ```powershell
//! cargo run -p latency-harness -- 127.0.0.1 --count 200 --rate 60 --qr "127.0.0.1#WPT1..."
//! ```

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use trackpad_core::{decode, encode, Message, DEFAULT_UDP_PORT};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn usage() -> ! {
    eprintln!(
        "usage: latency-harness [TARGET] [--port P] [--count N] [--rate HZ] [--qr \"<text>\"] [--qr-file PATH]"
    );
    eprintln!("  TARGET defaults to 127.0.0.1 (loopback self-test)");
    eprintln!("  --qr pairs first and measures the encrypted path");
    std::process::exit(2);
}

fn main() {
    let mut target = String::from("127.0.0.1");
    let mut port: u16 = DEFAULT_UDP_PORT;
    let mut count: usize = 200;
    let mut rate: u64 = 60;
    let mut qr: Option<String> = None;
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
            "--count" => {
                count = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--rate" => {
                rate = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--qr" => qr = Some(args.next().unwrap_or_else(|| usage())),
            "--qr-file" => {
                let path = args.next().unwrap_or_else(|| usage());
                qr = Some(
                    std::fs::read_to_string(&path)
                        .unwrap_or_else(|_| usage())
                        .trim()
                        .to_string(),
                );
            }
            s if s.starts_with('-') => usage(),
            s => target = s.to_string(),
        }
    }
    if count == 0 || rate == 0 {
        usage();
    }

    // Optional pairing: encrypted session keys for the v2 envelope.
    let keys = qr.as_deref().map(|qr_text| {
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|s| s.as_secs())
            .unwrap_or(0);
        let session = trackpad_desktop::pairing_client::pair_qr(
            qr_text,
            Some(&target),
            "latency-harness",
            now_unix,
        )
        .unwrap_or_else(|e| {
            eprintln!("latency-harness: pairing failed: {e}");
            std::process::exit(1);
        });
        port = session.udp_port;
        session.keys
    });

    let dest: SocketAddr = format!("{target}:{port}")
        .parse()
        .unwrap_or_else(|_| usage());
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap_or_else(|e| {
        eprintln!("latency-harness: cannot bind local socket: {e}");
        std::process::exit(1);
    });
    socket
        .set_read_timeout(Some(Duration::from_millis(1000)))
        .unwrap_or_else(|e| {
            eprintln!("latency-harness: cannot set timeout: {e}");
            std::process::exit(1);
        });

    let session_id = (now_ms() ^ (std::process::id() as u64)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let period = Duration::from_secs_f64(1.0 / rate as f64);
    let mut rtts: Vec<f64> = Vec::with_capacity(count);
    let mut lost = 0usize;
    let mut buf = [0u8; 128];

    for i in 0..count {
        let tick = Instant::now();
        let ts = now_ms();
        let ping = match &keys {
            Some(k) => trackpad_core::crypto::seal(
                &k.phone_key,
                session_id,
                i as u64,
                Message::Ping { timestamp_ms: ts },
            ),
            None => encode(session_id, i as u64, Message::Ping { timestamp_ms: ts }),
        };
        if let Err(e) = socket.send_to(&ping, dest) {
            eprintln!("latency-harness: send #{i} failed: {e}");
            lost += 1;
            continue;
        }
        match socket.recv_from(&mut buf) {
            Ok((len, _)) => {
                let msg: Result<Message, String> = match &keys {
                    Some(k) => trackpad_core::crypto::open(&k.desktop_key, &buf[..len])
                        .map(|(_, _, m)| m)
                        .map_err(|e| e.to_string()),
                    None => decode(&buf[..len])
                        .map(|pkt| pkt.msg)
                        .map_err(|e| e.to_string()),
                };
                match msg {
                    Ok(Message::Pong { timestamp_ms }) if timestamp_ms == ts => {
                        rtts.push(tick.elapsed().as_secs_f64() * 1000.0);
                    }
                    Ok(_) => {
                        eprintln!("latency-harness: reply #{i} is not the matching PONG");
                        lost += 1;
                    }
                    Err(e) => {
                        eprintln!("latency-harness: reply #{i} malformed: {e}");
                        lost += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("latency-harness: reply #{i} timeout ({e})");
                lost += 1;
            }
        }
        let elapsed = tick.elapsed();
        if elapsed < period {
            std::thread::sleep(period - elapsed);
        }
    }

    rtts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pct = |p: f64| -> f64 {
        if rtts.is_empty() {
            0.0
        } else {
            let idx = ((p / 100.0) * (rtts.len() - 1) as f64).round() as usize;
            rtts[idx.min(rtts.len() - 1)]
        }
    };
    let mean = if rtts.is_empty() {
        0.0
    } else {
        rtts.iter().sum::<f64>() / rtts.len() as f64
    };
    println!(
        "target: {dest}  sent: {count}  got: {}  lost: {lost}",
        rtts.len()
    );
    println!(
        "RTT ms: min {:.2}  mean {:.2}  p50 {:.2}  p95 {:.2}  max {:.2}",
        rtts.first().copied().unwrap_or(0.0),
        mean,
        pct(50.0),
        pct(95.0),
        rtts.last().copied().unwrap_or(0.0),
    );
    println!("note: one-way latency ~= RTT/2 on a symmetric path");
    if lost > 0 {
        std::process::exit(1);
    }
}
