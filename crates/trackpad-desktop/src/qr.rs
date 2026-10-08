//! Pairing QR: payload assembly + SVG rendering.
//!
//! The QR carries everything the phone needs without discovery (guest
//! Wi-Fi / AP isolation fallback): server fingerprint, candidate address,
//! ports, and the single-use secret. Rendered server-side as SVG so the
//! desktop UI needs no QR dependency.

use qrcode::QrCode;
use trackpad_core::pairing::{PairingSecret, QrPayload};

/// Primary LAN IPv4 address. The UDP `connect` trick resolves the route's
/// source address without sending any traffic.
pub fn primary_lan_ip() -> std::net::IpAddr {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0");
    if let Ok(s) = socket {
        if s.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = s.local_addr() {
                return addr.ip();
            }
        }
    }
    std::net::IpAddr::from([127, 0, 0, 1])
}

/// Assemble the QR text for a fresh ceremony.
pub fn qr_text(
    server_fp: &[u8; 32],
    tcp_port: u16,
    udp_port: u16,
    secret: &PairingSecret,
    device_name: &str,
) -> Result<String, trackpad_core::pairing::PairingError> {
    let ip = primary_lan_ip();
    let _ = ip; // The address is informational; the phone also tries mDNS.
    QrPayload {
        server_fp: *server_fp,
        tcp_port,
        udp_port,
        secret: *secret.bytes(),
        expires_unix: (secret.expires_at_ms() / 1000) as u32,
        device_name: device_name.to_string(),
    }
    .encode()
    .map(|payload| {
        // Prefix the routable address for networks where mDNS is blocked.
        // Format stays parseable: the phone splits at the first '#'.
        format!("{ip}#{payload}")
    })
}

/// Render QR SVG for the desktop UI (`<img src="data:image/svg+xml,...">`).
pub fn qr_svg(text: &str) -> Result<String, String> {
    let code = QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_text_round_trips_through_core_parser() {
        let secret = PairingSecret::from_bytes([5u8; 16], 1_000_000);
        let text = qr_text(&[9u8; 32], 51516, 51515, &secret, "pc").expect("qr");
        let (ip, payload) = text.split_once('#').expect("addr prefix");
        assert!(ip.parse::<std::net::IpAddr>().is_ok());
        let back = QrPayload::decode(payload, 0).expect("core parses our QR");
        assert_eq!(back.server_fp, [9u8; 32]);
        assert_eq!(back.secret, [5u8; 16]);
        assert_eq!(back.device_name, "pc");
    }

    #[test]
    fn svg_renders() {
        let svg = qr_svg("WPT1 test payload for rendering").expect("svg");
        assert!(svg.contains("<svg"));
    }
}
