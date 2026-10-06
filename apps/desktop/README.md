# Desktop agent

Not created yet. Phase 0 creates a Tauri 2 app here. Rust is not required to run `npm test` at the repo root.

The Rust codec must round-trip the golden bytes in `tests/protocol/wire.test.ts`. The webview must not call `SendInput` or `CGEvent`. Those calls live in the native adapter.
