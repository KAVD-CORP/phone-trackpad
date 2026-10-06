# AGENTS.md — Phone Trackpad

You are working on **Phone Trackpad**, a public KAVD Corp project: a phone used as a Windows and macOS trackpad. The unsolved problem is feel and setup friction, not "can we send a touch event". Read this file first.

## Read before coding

| Question | File |
|---|---|
| What we decided and what we rejected | `docs/TECH_STACK.md` |
| How packets, the desktop and the pad fit | `docs/ARCHITECTURE.md` |
| Byte layout | `docs/PROTOCOL.md` |
| Why Flutter, why Tauri, why UDP totals | `docs/adr/` |
| The original brief | `docs/reference/Wireless_Phone_Trackpad_Product_Proposal.docx` |
| Which prompt to run | `docs/PROMPTS.md` |
| First session for your role | `docs/KICKOFF_PROMPTS.md` |
| How to build | `CONTRIBUTING.md` |

## Stack (do not substitute)

Flutter (Dart 3) on Android and iOS. Tauri 2 on Windows and macOS. Cursor injection only through `SendInput` or `CGEvent`. UDP motion totals and TCP control messages as specified. Node's test runner for the protocol oracle. MIT license.

No React Native. No Electron. No .NET host. No browser trackpad. No kernel driver. No JSON on the wire. No absolute screen coordinates.

## Non-negotiable rules

1. **A lost motion packet must not shorten the swipe.** Totals are cumulative. The desktop applies the int32 gap. A stale sequence is ignored.
2. **A click must not travel on UDP.** Clicks, drags, and scrolls are TCP control messages.
3. **No input without a paired session.** Session id 0 is invalid. Revoking a device makes the next packet a no-op.
4. **The webview does not move the cursor.** Tauri's UI is settings and pairing. Rust calls the OS.
5. **Two ports, one byte layout.** If you change `docs/PROTOCOL.md`, you change `src/protocol`, the Node tests, and the Dart and Rust encoders in the same PR.
6. **Ask for macOS Accessibility. Do not skip it and do not hide the connected state.**
7. **Keyboard, media keys, and off-LAN control are out** until an ADR adds them.
8. **The first slice is Android + Windows.** Do not block it on iOS or macOS. Do not design it so those two require a rewrite.

## Conventions

- Protocol changes are a version bump, not a quiet extra byte.
- Phone code lives in `apps/phone`. Desktop code lives in `apps/desktop`.
- Conventional Commits. One phase per PR.
- Before finishing: `npm test`. Once an app exists, also run its tests.

## When unsure

ADRs beat the proposal. The proposal beats a guess about feel. If a change needs a new input driver or a new wire format, stop and write an ADR instead of coding it.
