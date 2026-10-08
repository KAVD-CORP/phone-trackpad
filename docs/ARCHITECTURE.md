# Architecture

## 1. Path

```text
[Flutter phone]
  finger  ->  unacked totals  ->  UDP motion (port 4620 / 51515)
  clicks, right-click, scroll, button up/down  ->  TCP (port 4622 / 51516)

        |  same Wi-Fi
        v

[Tauri desktop, Windows or macOS]
  motion: apply only the gap since the last accepted totals
  control: perform the click or scroll once
        |
        v
  Windows SendInput   or   macOS CGEvent
        |
        v
  the OS cursor, including a second monitor

  desktop acks the latest motion seq on UDP 4621
```

The webview inside Tauri is settings, pairing, and the connected status indicator. It is not on the pointer path.

## 2. Layers (strict boundaries)

| Layer | Lives in | Responsibility |
|---|---|---|
| Mobile UI | `apps/mobile/` | Touch surface, gestures, settings, pairing UI |
| Protocol Core | `crates/trackpad-core/` | Message types, encoding, encryption, sessions, pairing. No I/O assumptions |
| Desktop service | `crates/trackpad-desktop/` + `apps/desktop/src-tauri/` | Discovery advertising, networking, session management, coalescing |
| OS adapter | `crates/trackpad-desktop/` (`WindowsInjector`, `MacInjector`) behind `trait InputInjector` | Cursor/buttons/scroll via native APIs only (`SendInput` / `CGEvent`) |
| Bridge | `crates/trackpad-bridge/` | `flutter_rust_bridge` surface so phone uses the exact same core |
| Protocol Oracle | `src/protocol/`, `tests/protocol/` | TypeScript byte oracle and apply rules for reference verification |

## 3. Repo map

- `crates/trackpad-core/` — shared core: v1 codec, Noise pairing/reconnect, UDP AEAD, replay window, QR/SPAKE2 ceremony types
- `crates/trackpad-desktop/` — service + `InputInjector` + `MockInjector`, TCP pairing server, trust store (keyring + JSON), mDNS, QR assembly
- `crates/trackpad-bridge/` — FRB surface (codegen bindings in `apps/mobile/lib/src/rust/`); phone uses the exact same core
- `apps/mobile/` — Flutter app (onboarding/discovery/QR/code/trackpad)
- `apps/desktop/` — Tauri 2 app (pairing QR + code, approve/deny, devices, revoke; backend boots the same service lib as the binary)
- `src/protocol/` & `tests/protocol/` — TypeScript protocol specification and test suite
- `tools/latency-harness/` — plaintext + encrypted RTT measurement harness
- `tools/fake-phone/` — plaintext demos + full pair ceremony client
- `docs/PROTOCOL.md` — wire spec (v1 base layout + v2 encrypted envelope, pairing flows, QR, mDNS)

## 4. Input adapters

One trait in the desktop companion (`trait InputInjector`), two platform implementations:

| Impl | Platform | Mechanism |
|---|---|---|
| `WindowsInjector` | Windows 10/11 | `SendInput` for motion, left/right button, and vertical/horizontal wheel |
| `MacInjector` | macOS | `CGEvent` posted to the HID tap (requires macOS Accessibility permissions) |

No kernel drivers or third-party input drivers. The menu bar/tray shows a connected state whenever a session is active.

## 5. Session & Pairing

1. Desktop advertises on mDNS (`_phonetrackpad._tcp`) and displays a QR code plus a 6-digit short code.
2. Phone discovers desktop or scans QR / enters code.
3. Phone connects over TCP and completes Noise handshake (with PSK derived from QR secret or SPAKE2 short code).
4. Desktop assigns a non-zero session ID and derives directional ChaCha20-Poly1305 session keys via HKDF.
5. Only that session ID and encrypted payload is accepted on UDP. Out-of-window packets or unauthenticated sessions are dropped.
6. The user can revoke or disconnect the phone, immediately invalidating the session.

## 6. Latency budget

Measured on 5 GHz Wi-Fi:
- Time from finger move to cursor move: p50 under 16 ms, p95 under 40 ms.
- A dropped UDP datagram does not leave the cursor short: cumulative totals ensure the next datagram catches up the full motion gap (`applyMotion`).
- TCP clicks may be slightly slower than motion, but must be reliable and never dropped.

## 7. Failure behaviour

- Motion with a stale sequence: ignored.
- Motion with a newer sequence: move by the int32 gap in totals, even if sequence numbers jumped.
- Bad magic, wrong version, or session 0: dropped silently.
- TCP drop / disconnect: phone displays disconnected and stops sending motion.
- macOS Accessibility denied: companion alerts the user and does not inject input.

## 8. Decisions & Toolchain Pins

- Rust `1.90.0` (`rust-toolchain.toml`)
- Flutter `3.35.4` (`.flutter-version`)
- Node `22.17.1 LTS` (`.nvmrc`)
