# ADR-0005 — Keyboard mode

Status: Accepted (13 Nov 2026)

The product brief scoped this project to a trackpad. Keyboard input was deferred until an ADR added it. This is that ADR: the phone gains a second mode, chosen at app startup after pairing, that sends keystrokes to the paired desktop. It does not change what a motion packet is, and it does not introduce a second input stack.

## Decision

**Mode selection.** After a session is established, the phone offers a Trackpad screen and a Keyboard screen. Startup shows the choice; the user can switch between them at any time without re-pairing. The desktop does not care which screen the phone is on — the mode is a phone-side UI decision, not a session property. Packets in both modes carry the same paired session id, so rules 1 and 3 are untouched.

**Transport.** Keyboard events travel as control messages inside the existing encrypted session envelope — the same channel the shipped Phase 4 build already uses for clicks, buttons, and scroll (Amendment 13 Nov 2026: the draft said "TCP", but the implemented transport for critical input is the encrypted UDP envelope with guards, and keyboard follows it; a dedicated TCP control channel for all critical input stays future work). A lost key-up is a stuck key, which is user-visible, so keyboard state gets the same treatment buttons already have: senders carry the modifier bitmask on both the down and the up, key-ups are sent redundantly, and a desktop-side `KeyGuard` (mirror of `ButtonGuard`) releases anything still held after 500 ms of silence or a session change.

**Two new message types.**

| Message | Code | Payload | Delivery | Semantics |
|---|---|---|---|---|
| `TEXT` | `0x08` | UTF-8 chunk (max 512 bytes) | TCP | Typed characters; desktop injects them as Unicode input |
| `KEY` | `0x09` | `usage:u16, modifiers:u8, state:u8 (0=up, 1=down)` | TCP | Non-character keys: arrows, Esc, Enter, Backspace, Tab, F-keys |

`usage` is a **USB HID keyboard usage code**, not a per-platform code. The desktop maps HID usage → `VK_*` on Windows and → `CGKeyCode` on macOS. Modifiers are a bitmask: bit 0 Shift, bit 1 Ctrl, bit 2 Alt, bit 3 Meta.

**Injection driver.** Windows uses `SendInput` with `KEYBDINPUT`. macOS uses `CGEvent` keyboard events under the same Accessibility permission already requested for the mouse (ADR-0004). No other injection mechanism, no new permission beyond what pairing already asks for.

**Out of scope, unchanged.** Media keys stay deferred. Text prediction and IME composition stay on the phone's soft keyboard; the desktop only receives the committed characters.

## Consequences

- `docs/PROTOCOL.md` and the Dart and Rust encoders change in the same PR (rule 5). The envelope version stays `0x02`; the new messages are new types, not a new envelope. The TS oracle under `src/protocol` covers the v1 reference frames and is unchanged — the v2 logical-message oracle is `trackpad-core`'s Rust unit tests, same as for `MOVE`/`SCROLL`/`CLICK` today.
- The desktop needs a keyboard injector beside the mouse injector. It must reject `TEXT`/`KEY` for unpaired sessions exactly as it rejects motion (rule 3).
- Because keys are TCP, a stalled connection stalls typing rather than dropping it. That is the correct failure mode for text.
- The Flutter keyboard screen uses a text field for prose typing plus a fixed row for the keys the soft keyboard cannot emit (Esc, arrows, F-keys, modifiers).

## Alternatives considered

- **Keyboard over UDP.** Rejected: lost key-ups cause stuck keys, and key state is not recoverable from later packets the way cumulative motion totals recover a swipe.
- **Unicode text only, no `KEY` message.** Rejected: arrows, Esc, and F-keys are not text and are expected of a remote keyboard.
- **Per-platform key codes on the wire.** Rejected: HID usages are stable and platform-neutral; mapping belongs on the desktop side.
- **Tauri webview receives keys.** Rejected by rule 4 — the webview is settings and pairing, Rust calls the OS.
