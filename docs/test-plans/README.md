# Test Plans (Phase 0)

Automated tests in Phase 0: `cargo test` for the three crates (see Verification in README).
Device/lab checklists start in Phase 1. Placeholders for the files the spec requires:

- `manual-windows.md` — scaling 100/125/150%, multi-monitor, firewall prompt (starts Phase 1).
- `manual-macos.md` — Accessibility states: denied/granted/revoked-while-running (starts Phase 3).
- `manual-mobile.md` — iPhone 15 + Android sizes, permission denied, AP isolation (starts Phase 1).
- `manual-network.md` — congested Wi-Fi, sleep/wake, kill-mid-drag stuck-button test (starts Phase 2).

Phase 0 manual checklist (do this once on the dev machine):

1. `cargo test` passes on Rust 1.89.0.
2. `cargo run -p latency-harness` prints `latency-harness ok`.
3. After installing Flutter 3.35.4: `cd apps/mobile && flutter analyze && flutter test`.
4. After installing Tauri prereqs: `cd apps/desktop && npm install && npm run build`.
