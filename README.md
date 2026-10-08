# Wireless Phone Trackpad (Phase 0)

Phone-as-trackpad over LAN. See `PROMPT.md`, `docs/architecture.md`, `docs/protocol.md`.

Targets: Windows 10 (dev) + iPhone 15. MacBook on standby for Phase 3. No Apple Developer account yet.

## Layout

- `crates/trackpad-core/` — shared protocol core
- `crates/trackpad-desktop/` — service + `InputInjector` / `MockInjector`
- `crates/trackpad-bridge/` — mobile bridge stub
- `apps/mobile/` — Flutter app (needs Flutter 3.35.4)
- `apps/desktop/` — Tauri 2 app (needs Node 22 + Rust 1.89.0)
- `tools/latency-harness/` — measurement skeleton
- `docs/` — protocol draft, architecture, threat-model skeleton, test plans

## Verify (Windows 10, PowerShell)

```powershell
# Rust (install from https://rustup.rs/ if missing, toolchain 1.89.0)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p latency-harness -- 127.0.0.1   # needs trackpad-service running
cargo run -p trackpad-desktop --bin trackpad-service  # UDP 0.0.0.0:51515
cargo run -p fake-phone -- 127.0.0.1        # cursor glides right + clicks

# Desktop web UI (Node present)
cd apps/desktop; npm install; npm run build; cd ../..

# Mobile (after installing Flutter 3.35.4)
cd apps/mobile; flutter analyze; flutter test
```

## Status

Phase 5 polish (Windows): system tray (idle/active icon + tooltip, Show /
Pair / Quit, click-to-show), single-instance focus, start-on-login toggle,
NSIS installer (`npx tauri build`), error/empty states, iPhone sideload
guide. See `docs/packaging.md`, `docs/iphone-sideload.md`, `docs/setup.md`.
