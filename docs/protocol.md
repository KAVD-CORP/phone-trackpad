# Wire Protocol — v2 (Phase 4)

Status: pairing + encrypted transport implemented in
`crates/trackpad-core/src/{crypto,pairing}.rs`, desktop
`crates/trackpad-desktop/src/{pairing_server,pairing_client,trust,discovery,qr}.rs`,
phone via FRB (`crates/trackpad-bridge/src/api.rs`).
Plain v1 remains for `--open` LAN testing only; the service drops v1 by default.

## Message types (unchanged, still v1-framed inside)

MOVE `0x01`, CLICK `0x02`, PING `0x03`, PONG `0x04`, BUTTON_DOWN `0x05`,
BUTTON_UP `0x06`, SCROLL `0x07`. See §2 (frozen since Phase 2).

## Encrypted UDP envelope (version byte `0x02`)

`version:u8=2 | session:u64 LE | seq:u64 LE | type:u8 | ciphertext | tag:16`

- ChaCha20-Poly1305, 256-bit directional keys (phone→desktop, desktop→phone).
- Nonce: 12 bytes = `0x00000000 || seq LE`. Safe because keys are fresh
  per handshake and `seq` is strictly increasing per session.
- AAD covers the full 18-byte header (version/session/seq/type).
- Decrypted payload is re-framed as v1 and run through the existing strict
  decoder, so all v1 validity rules apply unchanged.
- Receivers enforce a 1024-packet sliding replay window (`ReplayWindow`):
  duplicates and out-of-window packets are dropped silently.
- snow's `TransportState` is deliberately NOT used for UDP: its sequential
  nonce desyncs on lossy transport.

## Session keys

`HKDF-SHA256(handshake_hash)` with info `wpt-udp-phone-v1` /
`wpt-udp-desktop-v1` (32 bytes each). Rekeyed on every handshake;
reconnect derives fresh keys even for known devices.

## Pairing (TCP, default port 51516)

Framing: `u16-BE length + bytes`, max 8 KiB per frame, 15 s per step.
Hellos: `PAIR-QR`, `PAIR-SPAKE2`, `RECONNECT`.

1. **QR:** desktop generates a random 128-bit secret (TTL 2 min, consumed
   on approval success) and shows `ip#base64url(...)` plus a 6-digit code.
   Phone connects, both run `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s` with
   `PSK = HKDF(secret, "wpt-pair-psk-v1")`. The phone checks the responder
   static against the QR fingerprint *before* sending its device name.
2. **Short code:** SPAKE2 (6 digits) over two frames derives the PSK;
   the same XXpsk3 exchange follows, bound to the mDNS-advertised
   fingerprint. Neither SPAKE2-`finish` nor Noise message 2 detects a wrong
   secret — authentication lands at Noise message 3. Attempt accounting
   counts Noise failures (3 strikes → 30 s lockout, server-wide).
3. **Approval:** the desktop parks the completed handshake (60 s) for
   explicit human approval showing the phone-supplied device name
   (sanitized, 64 chars). Approve stores trust + publishes the UDP
   session; deny/timeout closes with no trust stored.
4. **Reconnect:** `Noise_XX_25519_ChaChaPoly_Blake2s`, no PSK. The peer
   static must already be trusted; session publishes immediately.

## QR payload (binary, base64url no-pad, `#`-prefixed with the LAN IP)

`WPT1 | ver u8=1 | server_fp[32] | tcp u16BE | udp u16BE | secret[16] |
expires u32BE unix | name_len u8 | name`. The `ip#` prefix is the
AP-isolation fallback (mDNS often blocked on guest Wi-Fi).

## Discovery (mDNS/DNS-SD)

Desktop advertises `<name>._wptrackpad._tcp.local`, TXT
`ver=2, udp=<port>, fp=<base64url-fp>`. No secrets in TXT. Phones browse
for the onboarding list; QR/manual entry covers blocked multicast.

## 1. Design rules (from product spec, non-negotiable)

- Relative movement only (`MOVE{dx,dy}`). Never absolute coordinates.
- Compact binary on hot path. No JSON for input packets.
- Layout per UDP packet: `version:u8 | session_id:u64 | seq:u64 | msg_type:u8 | payload:bytes | AEAD tag`.
- Nonce derived from sequence number (ChaCha20-Poly1305, fresh session key per connection).
- Sliding replay window: reject duplicates + out-of-window packets silently.
- `MOVE` / `SCROLL` are lossy + coalescible: apply newest, sum deltas within one injection tick, never burst-catch-up.
- Button events are critical: repeat 2–3x with same id (or ACK); desktop releases held buttons on session end / silence timeout / revoke.
- Reject malformed/oversized packets without panic. Decoder gets fuzzed (`cargo-fuzz` / `proptest` in Phase 2).
- Phone sends at display cadence (60–120 Hz), capped rate, nothing when idle. Sub-pixel remainders carried forward.

## 2. Logical messages (v1: implemented)

Wire: `version:u8 | session_id:u64 LE | seq:u64 LE | msg_type:u8 | payload`.

| Name | Type | Payload | Channel | Reliability |
|---|---|---|---|---|
| `MOVE` | `0x01` | `dx:i16 LE, dy:i16 LE` | UDP | lossy, coalesced per 8 ms tick |
| `CLICK` | `0x02` | `button:u8` (`0` left, `1` right) | UDP | best-effort single shot |
| `PING` | `0x03` | `timestamp_ms:u64 LE` | UDP | answered with `PONG` |
| `PONG` | `0x04` | `timestamp_ms:u64 LE` (echo) | UDP | matches PING by timestamp |
| `BUTTON_DOWN` | `0x05` | `button:u8` | UDP | critical: flush-before-apply; desktop releases on session change / 500 ms silence |
| `BUTTON_UP` | `0x06` | `button:u8` | UDP | critical: senders repeat 2x; same desktop guards as DOWN |
| `SCROLL` | `0x07` | `dx:i16 LE, dy:i16 LE` finger px | UDP | lossy like MOVE; desktop emits wheel notches at 40 px |

Button ids: `0` left, `1` right, `2` middle (`trackpad-core::button`).
Unknown button ids are ignored by `dispatch` (forward-compat).

Strictness: max datagram 64 bytes; unknown version/type, short reads, and
trailing bytes are rejected (`DecodeError`, no panics). Biggest Phase 2
packet is 26 bytes (PING/PONG).

Desktop behavior (see `crates/trackpad-desktop/src/service.rs`):
- MOVE/SCROLL accumulate in `Coalescer`, flushed every 8 ms and before any
  button/click/ping handling, so drag order (DOWN → moves → UP) is kept.
- `ButtonGuard` releases held buttons when the `session_id` changes or no
  packet arrives for 500 ms (`SILENCE_TIMEOUT_MS`).
- SCROLL pixels convert to wheel events at 40 px per 120-unit notch
  (`ScrollAccum`); natural/inverted is applied sender-side in the phone
  gesture engine.

Strictness: max datagram 64 bytes; unknown version/type, short reads, and
trailing bytes are rejected (`DecodeError`, no panics). `MAX_PACKET_LEN`
covers the 26-byte max (PING/PONG) with headroom; frozen at 64 for Phase 2.

## 3. Open questions for Phase 4

1. `seq` width (u64 today) vs overhead at 120 Hz; replay-window sizing.
2. Whether `CLICK` stays or becomes sugar over DOWN+UP (kept: tap path in
   the gesture engine emits CLICK directly; drag path uses DOWN/UP).
3. Session-key rotation interval on top of the current `session_id` switch.
