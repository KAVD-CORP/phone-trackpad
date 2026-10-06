# ADR-0002 — Tauri 2 for the desktop

Status: Accepted (6 Oct 2026)

Phone Controller's host is .NET because the virtual gamepad is Windows-only. This product must move a cursor on Windows and macOS and sit in the tray or menu bar while the user is not looking at it.

Tauri 2 is the shell. Rust owns sockets, the session, and the input adapters (`SendInput`, `CGEvent`). The webview owns settings and pairing UI only.

Electron is rejected for the idle footprint. A pure .NET agent is rejected because macOS input and the menu bar are the awkward half. Flutter on the desktop is rejected for the same reason. A kernel driver is rejected. If the Accessibility permission is missing, the agent asks for it and does not inject input.
