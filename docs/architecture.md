# Architecture (Phase 0)

```
Phone (Flutter)                    LAN                   Desktop (Tauri 2)
 Touch surface ─┐                                        ┌─ Tray + settings (TS)
 Gesture engine ├─ UDP: input ─────────────────────────► │ Service (Rust)
 Settings/pair ─┘    TCP: control                        │  discovery, sessions
   trackpad-core ◄── shared crate ──► trackpad-core ─────┤  InputInjector (native)
```

## Layers (strict boundaries)

| Layer | Lives in | Responsibility |
|---|---|---|
| Mobile UI | `apps/mobile/` | touch surface, gestures, settings, pairing UI |
| Protocol | `crates/trackpad-core/` | message types, encoding, encryption, sessions, pairing. No I/O assumptions where possible |
| Desktop service | `crates/trackpad-desktop/` + `apps/desktop/src-tauri/` | discovery advertising, networking, session mgmt, coalescing |
| OS adapter | `crates/trackpad-desktop/` (`WindowsInjector`, `MacInjector`) behind `trait InputInjector` | cursor/buttons/scroll via native APIs only |
| Bridge | `crates/trackpad-bridge/` | `flutter_rust_bridge` surface so phone uses the exact same core |

## Repo map

- `crates/trackpad-core/` — shared core: v1 codec, Noise pairing/reconnect,
  UDP AEAD, replay window, QR/SPAKE2 ceremony types
- `crates/trackpad-desktop/` — service + `InputInjector` + `MockInjector`,
  TCP pairing server, trust store (keyring + JSON), mDNS, QR assembly
- `crates/trackpad-bridge/` — FRB surface (codegen bindings in
  `apps/mobile/lib/src/rust/`); phone uses the exact same core
- `apps/mobile/` — Flutter app (onboarding/discovery/QR/code/trackpad)
- `apps/desktop/` — Tauri 2 app (pairing QR + code, approve/deny, devices,
  revoke; backend boots the same service lib as the binary)
- `tools/latency-harness/` — plaintext + `--qr-file` encrypted RTT
- `tools/fake-phone/` — plaintext demos + full `pair` ceremony client
- `docs/protocol.md` — wire spec (v2 envelope, pairing flows, QR, mDNS)

## Phase 0 decisions

- Dev machine: Windows 10 + iPhone 15. Phase 1 builds Windows injector first; macOS injector in Phase 3 using side MacBook.
- No Apple Developer account yet: iOS testing via sideload/TestFlight deferred; Phase 1 verifies with iPhone on LAN to Windows host.
- Toolchains pinned: Rust `1.89.0` (`rust-toolchain.toml`), Flutter `3.35.4` (`.flutter-version`), Node `22.17.1 LTS` (`.nvmrc`). CI uses same versions.
