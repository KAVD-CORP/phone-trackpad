# Phone Trackpad

Use your phone as a trackpad for a Windows or macOS computer. The hard part is not sending a touch. The hard part is a cursor that still feels continuous, with none of the setup a remote-control utility usually demands.

A **public** KAVD Corp repository. MIT license. Outside contributions are welcome.

## Start here

1. `AGENTS.md` — rules and core conventions.
2. `docs/TECH_STACK.md` — Flutter on the phone, Tauri on the desktop, and options rejected.
3. `docs/ARCHITECTURE.md` — packet path, session state, latency budget.
4. `docs/PROTOCOL.md` — wire protocol (v1 motion totals + v2 encrypted transport).
5. `docs/adr/` — Architecture Decision Records.
6. `docs/reference/Wireless_Phone_Trackpad_Product_Proposal.docx` — the original brief.
7. `docs/KICKOFF_PROMPTS.md` — kickoff prompts for team roles.
8. `docs/PROMPTS.md` — phase prompts.

## Layout

| Path | Description |
|---|---|
| `crates/trackpad-core/` | Shared protocol core: codecs, Noise pairing, ChaCha20 AEAD, replay window |
| `crates/trackpad-desktop/` | Background service, `InputInjector` (Windows `SendInput`, macOS `CGEvent`), QR/mDNS |
| `crates/trackpad-bridge/` | `flutter_rust_bridge` FFI bindings for the mobile app |
| `apps/desktop/` | Tauri 2 desktop app (React + Vite + Rust tray companion) |
| `apps/mobile/` | Flutter app for iOS and Android |
| `src/protocol/` | TypeScript wire format codec and motion apply oracle |
| `tests/protocol/` | Golden byte test suite |
| `tools/` | Fake phone simulator, latency harness, web gesture test |
| `docs/` | Architecture, protocol spec, test plans, threat model, packaging |

## Verification & Testing

### Protocol Oracle (Node.js)
```bash
npm test
```

### Rust Core & Desktop Service
```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### Running Tools & Service
```powershell
# Run desktop background service (UDP 51515, TCP 51516)
cargo run -p trackpad-desktop --bin trackpad-service

# Run latency measurement harness
cargo run -p latency-harness -- 127.0.0.1

# Run simulated phone movement & click test
cargo run -p fake-phone -- 127.0.0.1
```

### Desktop App (Tauri 2 / Web)
```powershell
cd apps/desktop
npm install
npm run build
# Or run dev mode (requires Tauri prerequisites):
npx tauri dev
```

### Mobile App (Flutter)
```powershell
cd apps/mobile
flutter analyze
flutter test
```

## Status

Phase 0–5 implementations completed:
- Core wire protocol & TypeScript reference test suite
- Rust shared core with Noise handshake, ChaCha20-Poly1305 encryption, and replay protection
- Windows desktop companion with system tray, QR code generation, and native `SendInput`
- Flutter mobile application with touch surface, gesture recognition, and mDNS discovery
