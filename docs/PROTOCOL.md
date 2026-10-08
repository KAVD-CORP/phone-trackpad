# Wire Protocol Spec

Little-endian encoding. The protocol specifies both the reference baseline (v1 cumulative totals) and the production transport (v2 encrypted AEAD envelope + Noise pairing).

Ports: UDP **4620 / 51515** (motion/input), UDP **4621** (ack), TCP **4622 / 51516** (pairing and control).

---

## 1. Reference Motion Protocol (v1 Wire Format)

Three core frames for unencrypted reference and testing (`src/protocol/wire.ts`, `tests/protocol/wire.test.ts`).

### Motion — 20 bytes — Magic `TP`

| Offset | Type | Field |
|---|---|---|
| 0 | u8 | `T` (0x54) |
| 1 | u8 | `P` (0x50) |
| 2 | u8 | version = 1 |
| 3 | u8 | flags (0 in v1) |
| 4 | u32 | session (never 0) |
| 8 | u32 | sequence |
| 12 | i32 | dx cumulative total since session start |
| 16 | i32 | dy cumulative total since session start |

`dx` and `dy` are **cumulative totals**, not deltas. The desktop host applies only the gap from the last accepted total with int32 wrap. A lost datagram is recovered when a later one arrives. Stale sequence numbers are dropped.

**Golden vector** (session `0x01020304`, seq 7, dx 12, dy -4):
```text
54 50 01 00 04 03 02 01 07 00 00 00 0c 00 00 00 fc ff ff ff
```

### Ack — 12 bytes — Magic `TA`

| Offset | Type | Field |
|---|---|---|
| 0 | u8 | `T` (0x54) |
| 1 | u8 | `A` (0x41) |
| 2 | u8 | version = 1 |
| 3 | u8 | reserved (0) |
| 4 | u32 | session |
| 8 | u32 | highest motion sequence applied |

### Control — 18 bytes — Magic `TC` (TCP only)

| Offset | Type | Field |
|---|---|---|
| 0 | u8 | `T` (0x54) |
| 1 | u8 | `C` (0x43) |
| 2 | u8 | version = 1 |
| 3 | u8 | kind (1: LeftClick, 2: RightClick, 3: ButtonDown, 4: ButtonUp, 5: Scroll) |
| 4 | u32 | session |
| 8 | u32 | sequence |
| 12 | u8 | button (0: left, 1: right, 2: middle) |
| 13 | u8 | reserved (0) |
| 14 | i16 | scroll dx (or 0) |
| 16 | i16 | scroll dy (or 0) |

---

## 2. Encrypted Transport (v2 Envelope)

Implemented in `crates/trackpad-core/src/{crypto,pairing}.rs` and `crates/trackpad-desktop/`.

### Envelope Structure (Version Byte `0x02`)
```text
version:u8=2 | session:u64 LE | seq:u64 LE | type:u8 | ciphertext | tag:16
```

- **Cipher**: ChaCha20-Poly1305 with directional 256-bit keys (phone → desktop, desktop → phone).
- **Nonce**: 12 bytes (`0x00000000 || seq LE`). Keys are fresh per handshake; `seq` is strictly monotonic.
- **AAD**: Covers the full 18-byte header (`version / session / seq / type`).
- **Replay Protection**: 1024-packet sliding window (`ReplayWindow`). Duplicates and out-of-window packets are dropped silently.
- **Session Keys**: `HKDF-SHA256(handshake_hash)` with info `wpt-udp-phone-v1` / `wpt-udp-desktop-v1` (32 bytes each). Rekeyed on every handshake.

---

## 3. Pairing & Handshake (TCP 51516)

Framing: `u16-BE length + bytes`, max 8 KiB per frame, 15 s per step timeout.

1. **QR Code Pairing (`PAIR-QR`)**:
   Desktop generates a 128-bit secret (TTL 2 min) and displays `ip#base64url(...)` + 6-digit short code.
   Phone connects over TCP; both execute `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s` with `PSK = HKDF(secret, "wpt-pair-psk-v1")`.
   Phone verifies responder public key fingerprint matches QR before sending its name.

2. **Short-Code Pairing (`PAIR-SPAKE2`)**:
   SPAKE2 exchange (6 digits) over two frames derives the PSK, followed by `Noise_XXpsk3` bound to the mDNS fingerprint.
   Rate-limiting lock: 3 consecutive failed attempts triggers a 30 s server-wide lockout.

3. **Approval**:
   Desktop parks the completed handshake (up to 60 s) for explicit user approval showing the phone's device name.
   On approval: peer static key is saved to the trust store and the live UDP session is activated.

4. **Reconnection (`RECONNECT`)**:
   `Noise_XX_25519_ChaChaPoly_BLAKE2s` without PSK. Peer public key must already exist in the trust store.

### QR Payload Format
```text
WPT1 | ver:u8=1 | server_fp[32] | tcp:u16BE | udp:u16BE | secret[16] | expires:u32BE unix | name_len:u8 | name
```

### mDNS / DNS-SD Service
Desktop advertises `<name>._wptrackpad._tcp.local` with TXT `ver=2, udp=<port>, fp=<base64url-fp>`.

---

## 4. Logical Messages & Input Semantics

| Message | Code | Payload | Delivery | Semantics |
|---|---|---|---|---|
| `MOVE` | `0x01` | `dx:i16, dy:i16` | UDP | Coalesced per 8 ms tick; relative pointer motion |
| `CLICK` | `0x02` | `button:u8` | UDP/TCP | Button click (0=left, 1=right) |
| `PING` | `0x03` | `timestamp:u64` | UDP | Echo probe for RTT latency measurement |
| `PONG` | `0x04` | `timestamp:u64` | UDP | Echo response |
| `BUTTON_DOWN` | `0x05` | `button:u8` | UDP/TCP | Press and hold button (drag gesture) |
| `BUTTON_UP` | `0x06` | `button:u8` | UDP/TCP | Release held button |
| `SCROLL` | `0x07` | `dx:i16, dy:i16` | UDP/TCP | Two-finger scroll; desktop converts to wheel notches |

- **Coalescing**: Moves and scrolls accumulate in `Coalescer` and flush every 8 ms to prevent event queue saturation.
- **ButtonGuard**: Desktop automatically releases any held buttons if session changes or if silence exceeds 500 ms (`SILENCE_TIMEOUT_MS`).
- **Scroll Accumulation**: 40 px per 120-unit standard OS wheel notch (`ScrollAccum`). Natural scrolling applied sender-side.
