//! Pairing client used by `fake-phone pair` (and tests) to drive the phone
//! side of the ceremonies against a real server. The production phone app
//! talks to the same protocol through `flutter_rust_bridge`, not this
//! module — but the bytes on the wire are identical.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use trackpad_core::crypto::{derive_udp_keys, NoiseHandshake, UdpKeys};
use trackpad_core::pairing::{PairingError, QrPayload};

const STEP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("network failed")]
    Io(#[from] std::io::Error),
    #[error("crypto failed")]
    Crypto,
    #[error("server fingerprint does not match the QR code")]
    FingerprintMismatch,
    #[error("server denied pairing: {0}")]
    Denied(String),
    #[error("bad QR: {0}")]
    BadQr(#[from] PairingError),
}

fn write_frame(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    stream.write_all(&(data.len() as u16).to_be_bytes())?;
    stream.write_all(data)
}

fn read_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
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

/// An approved pairing: session keys plus where to send input.
pub struct PairedSession {
    pub keys: UdpKeys,
    pub udp_port: u16,
    pub session_id_hint: u64,
}

/// Run the QR ceremony. `qr_text` is the full scanned string (`ip#payload`
/// or bare payload, in which case `tcp_host` is used). Blocks until the
/// desktop approves (or denies/times out).
pub fn pair_qr(
    qr_text: &str,
    tcp_host_override: Option<&str>,
    device_name: &str,
    now_unix: u64,
) -> Result<PairedSession, ClientError> {
    let (host, payload) = match qr_text.split_once('#') {
        Some((h, p)) => (h.to_string(), p),
        None => (
            tcp_host_override.unwrap_or("127.0.0.1").to_string(),
            qr_text,
        ),
    };
    let qr = QrPayload::decode(payload, now_unix)?;
    let addr = format!("{}:{}", host, qr.tcp_port)
        .to_socket_addrs()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad address"))?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "unresolvable"))?;
    let psk = crate::pairing_server::stretch_psk(&qr.secret);

    let (client_priv, _) =
        trackpad_core::crypto::generate_keypair().map_err(|_| ClientError::Crypto)?;
    let mut hs =
        NoiseHandshake::initiator_pair(&client_priv, &psk).map_err(|_| ClientError::Crypto)?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    stream.set_read_timeout(Some(STEP_TIMEOUT))?;

    write_frame(&mut stream, b"PAIR-QR")?;
    exchange(&mut stream, &mut hs, device_name, Some(&qr.server_fp))?;
    finish_pair(&mut stream, &hs)
}

/// Reconnect with a previously trusted keypair.
pub fn reconnect(
    tcp_addr: &str,
    client_priv: &[u8],
    server_fp: &[u8; 32],
    device_name: &str,
) -> Result<PairedSession, ClientError> {
    let addr = tcp_addr
        .to_socket_addrs()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad address"))?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "unresolvable"))?;
    let mut hs =
        NoiseHandshake::initiator_reconnect(client_priv).map_err(|_| ClientError::Crypto)?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    stream.set_read_timeout(Some(STEP_TIMEOUT))?;
    write_frame(&mut stream, b"RECONNECT")?;
    exchange(&mut stream, &mut hs, device_name, Some(server_fp))?;
    finish_pair(&mut stream, &hs)
}

/// The 3-message Noise exchange. Verifies the responder static against
/// `expect_fp` when given (QR/short-code binding).
fn exchange(
    stream: &mut TcpStream,
    hs: &mut NoiseHandshake,
    device_name: &str,
    expect_fp: Option<&[u8; 32]>,
) -> Result<(), ClientError> {
    let m1 = hs.write_message(&[]).map_err(|_| ClientError::Crypto)?;
    write_frame(stream, &m1)?;
    let m2 = read_frame(stream)?;
    if m2.starts_with(b"ERR") {
        return Err(ClientError::Denied(
            String::from_utf8_lossy(&m2).into_owned(),
        ));
    }
    hs.read_message(&m2).map_err(|_| ClientError::Crypto)?;
    if let Some(fp) = expect_fp {
        let remote = hs.remote_static().ok_or(ClientError::Crypto)?;
        if remote.as_slice() != fp.as_slice() {
            return Err(ClientError::FingerprintMismatch);
        }
    }
    let m3 = hs
        .write_message(device_name.as_bytes())
        .map_err(|_| ClientError::Crypto)?;
    write_frame(stream, &m3)?;
    Ok(())
}

/// After the exchange: pairing waits for approval (PENDING then OK/DENIED),
/// reconnect gets OK immediately.
fn finish_pair(stream: &mut TcpStream, hs: &NoiseHandshake) -> Result<PairedSession, ClientError> {
    let first = read_frame(stream)?;
    if first.starts_with(b"ERR") {
        return Err(ClientError::Denied(
            String::from_utf8_lossy(&first).into_owned(),
        ));
    }
    if first == b"DENIED" {
        return Err(ClientError::Denied("denied".to_string()));
    }
    let ok_line = if first == b"PENDING" {
        let second = read_frame(stream)?;
        if second == b"DENIED" || second.starts_with(b"ERR") {
            return Err(ClientError::Denied(
                String::from_utf8_lossy(&second).into_owned(),
            ));
        }
        second
    } else {
        first
    };
    let text =
        String::from_utf8(ok_line).map_err(|_| ClientError::Denied("bad OK frame".to_string()))?;
    let mut parts = text.split_whitespace();
    if parts.next() != Some("OK") {
        return Err(ClientError::Denied(text));
    }
    let udp_port: u16 = parts
        .next()
        .and_then(|p| p.parse().ok())
        .ok_or_else(|| ClientError::Denied("bad port".to_string()))?;
    let session_id_hint: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let keys = derive_udp_keys(&hs.handshake_hash()).map_err(|_| ClientError::Crypto)?;
    Ok(PairedSession {
        keys,
        udp_port,
        session_id_hint,
    })
}
