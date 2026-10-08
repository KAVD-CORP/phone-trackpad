//! LAN discovery: advertise the pairing TCP service via mDNS/DNS-SD.
//!
//! Instance: `<device-name>._wptrackpad._tcp.local.`, TXT records carry the
//! protocol version and UDP port. No sensitive data is advertised — the
//! pairing secret travels only inside the QR code / short-code ceremony.

/// mDNS service type (both UDP/TCP forms registered by convention; the
/// pairing channel is TCP).
pub const SERVICE_TYPE: &str = "_wptrackpad._tcp.local.";
pub const PROTOCOL_TXT_VERSION: &str = "2";

/// Registered advertisement. Held as long as discovery should run.
pub struct Advertiser {
    daemon: mdns_sd::ServiceDaemon,
    fullname: String,
}

impl Advertiser {
    /// Advertise `device_name` with the TCP pairing port, UDP input port,
    /// and the server fingerprint (binds short-code pairing when the QR
    /// cannot be scanned; the fingerprint is public).
    pub fn start(
        device_name: &str,
        server_fp: &[u8; 32],
        tcp_port: u16,
        udp_port: u16,
    ) -> Result<Self, mdns_sd::Error> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let daemon = mdns_sd::ServiceDaemon::new()?;
        let udp = udp_port.to_string();
        let fp = URL_SAFE_NO_PAD.encode(server_fp);
        let props = [("ver", PROTOCOL_TXT_VERSION), ("udp", &udp), ("fp", &fp)];
        let info = mdns_sd::ServiceInfo::new(
            SERVICE_TYPE,
            device_name,
            &format!("{device_name}.local."),
            (),
            tcp_port,
            &props[..],
        )?;
        let fullname = info.get_fullname().to_string();
        daemon.register(info)?;
        Ok(Self { daemon, fullname })
    }

    pub fn fullname(&self) -> &str {
        &self.fullname
    }
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}
