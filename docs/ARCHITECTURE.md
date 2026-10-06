# Architecture

## 1. Path

```
[Flutter phone]
  finger  ->  unacked totals  ->  UDP motion (20 bytes, port 4620)
  clicks, right-click, scroll, button up/down  ->  TCP (port 4622)

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

The webview inside Tauri is settings, pairing, and the connected dot. It is not on the pointer path.

## 2. Repository

```
src/protocol/          TypeScript codec and the apply rule. The spec is docs/PROTOCOL.md.
tests/protocol/        Golden bytes and loss behaviour. Dart and Rust must match.
apps/phone/            Flutter app. Created in Phase 0.
apps/desktop/          Tauri agent. Created in Phase 0. Input adapters live here.
docs/                  Stack, protocol, ADRs, prompts.
docs/reference/        Product proposal.
```

## 3. Input adapters

One trait in the desktop app, two implementations:

| Impl | When |
|---|---|
| `WindowsInput` | `SendInput` for move, left/right button, vertical and horizontal wheel. |
| `MacosInput` | `CGEvent` posted to the HID tap. The process must hold Accessibility. If it does not, move nothing and show the permission screen. |

Do not add a third backend without an ADR. Do not hide the cursor injection. The menu bar shows a connected state whenever a session is live.

## 4. Session

1. Desktop advertises on mDNS (`_phonetrackpad._tcp`) and shows a QR plus a short code.
2. Phone joins TCP 4622 and presents the code.
3. Desktop assigns a non-zero session id.
4. Only that session id is accepted on UDP. Anything else is dropped.
5. The user can forget the phone. The next datagram from it does nothing.

Before a packaged release, motion and control datagrams also carry a MAC (Phase 4). The bootstrap codec checks identity and loss behaviour first.

## 5. Latency budget

Measured on one mid-range Android phone and a Windows laptop, same room, 5 GHz:

- Time from finger move to cursor move: p50 under 16 ms, p95 under 40 ms.
- A dropped UDP datagram does not leave the cursor short. The test for that is `applyMotion` in `tests/protocol/wire.test.ts`.
- TCP clicks may be slower than motion. They may not be lost.

## 6. Failure behaviour

- Motion with a stale sequence: ignore.
- Motion with a newer sequence: move by the int32 gap in the totals, even if the sequence jumped.
- Bad magic, wrong version, or session 0: drop.
- TCP drops: the phone shows disconnected and stops sending motion. It does not keep sliding the cursor.
- macOS Accessibility denied: the agent stays open and does not pretend to be connected.
