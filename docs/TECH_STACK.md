# Phone Trackpad — Technology stack

Owner: Bubu (CTO). Status: **Adopted** (6 Oct 2026). Changes go through an ADR in `docs/adr/`.

Source of the product vision: `docs/reference/Wireless_Phone_Trackpad_Product_Proposal.docx`. This file is the decision. The proposal is the brief, and its own stack section says it is a recommendation, not a hard requirement.

This repository is **public**. Contributors will not all be KAVD. The stack has to be something a stranger can build, and it has to survive the actual constraints: a cursor that still feels continuous on ordinary Wi-Fi, a desktop agent on **Windows and macOS**, and a phone app on iOS and Android.

## 1. Decision in one paragraph

**Flutter (Dart) on the phone. Tauri 2 (Rust) on the desktop, with a small TypeScript tray UI. Native input only: Windows `SendInput`, macOS `CGEvent`. Pointer motion is a 20-byte UDP total, not a text line and not an absolute screen point. Clicks and scrolls go on a reliable TCP channel.** Not one language. The phone problem is a full-screen gesture surface. The desktop problem is a menu-bar agent that is allowed to move the cursor on two operating systems. The shared thing is the packet, tested in `src/protocol`.

## 2. Summary

| Layer | Choice | Why |
|---|---|---|
| Phone | **Flutter stable, Dart 3** | One gesture surface for iOS and Android. The product is a trackpad, not a form. |
| Desktop shell | **Tauri 2** | Windows tray and macOS menu bar from one project, small enough to leave running. The webview draws settings. It does not move the cursor. |
| Desktop input | **Native adapters** | `SendInput` on Windows. `CGEvent` on macOS, which requires the Accessibility permission the user grants. No kernel driver. |
| Motion | **UDP port 4620, 20 bytes, cumulative totals** | A lost datagram must not shorten the swipe. The next packet still contains the movement. See `docs/PROTOCOL.md`. |
| Clicks, drags, scrolls | **TCP port 4622** | A lost click is a bug. A lost move is caught up. They are different. |
| Acks | **UDP port 4621, 12 bytes** | The phone drops totals the desktop has already applied. |
| Discovery | **mDNS**, with a typed host as the fallback | The brief forbids a normal setup that starts with an IP address. Some networks block multicast, so the fallback exists. |
| Pairing | **QR or short code, then a session id** | No account. No cloud for local use. Session `0` is rejected. A MAC on each datagram is required before a release build (Phase 4). |
| Protocol oracle | `src/protocol` (TypeScript) + `tests/protocol` | Rust and Dart are not on the machine that bootstrapped this repo. Both ports must match `tests/protocol/wire.test.ts`, including the golden bytes. |
| CI | GitHub Actions, `npm test` | Rust and Flutter CI are added in the PR that adds each app. |
| License | **MIT** | No CLA. |

## 3. Options we weighed

| Option | What you gain | Why it is not the start |
|---|---|---|
| **.NET desktop, as on Phone Controller** | Allan already has a C# host next door. | That host is Windows-only because VIIPER is Windows-only. This product's desktop targets are Windows **and** macOS. A C# menu-bar agent fighting `CGEvent` is the worse half. |
| **Electron** | The team already ships TypeScript. | A tray process that stays running does not get a Chromium. The brief calls this out. |
| **Flutter on the desktop too** | One language with the phone. | A Flutter menu-bar agent is a poor fit, and cursor injection is still a native call. |
| **React Native** | KAVD already ships React. | The screen is a gesture surface. The same reason Phone Controller rejected it. |
| **Kotlin + Swift, and WinUI + AppKit** | The best permission and input APIs. | Four apps for a four-person team and for outside contributors. Native code is the input adapter, not the whole product. |
| **Browser page** | Nothing to install on the phone. | A page cannot move the desktop cursor, and it cannot be the paired device the user trusts. |
| **Text messages (`MOVE dx=12`)** | Easy to read in a log. | The brief sketches that line. It is not a protocol. Parsing, partial lines, and ambiguity do not belong on the pointer path. |
| **Absolute screen coordinates** | Simple mapping. | Breaks the moment the laptop has two monitors or a different scale. A trackpad sends relative motion. The OS places the cursor. |
| **TCP for the pointer** | No lost moves. | One delayed packet stalls every later one. Cumulative UDP totals recover a loss without that stall. |
| **Our own kernel driver** | Input that does not ask permission. | Out. macOS Accessibility and Windows `SendInput` are the supported doors. The app shows when it is connected. |

## 4. What “shared” actually means

```
phone (Dart)  --UDP motion totals-->  desktop (Rust)  --SendInput / CGEvent-->  OS cursor
phone (Dart)  <--UDP ack--            desktop (Rust)
phone (Dart)  --TCP click / scroll--> desktop (Rust)
```

Sensitivity and natural scrolling are applied on the phone before a total is updated. The desktop moves one-to-one with the gap it computes.

## 5. Sampling, stated honestly

Flutter's gesture stream is not a fixed clock. The rule is:

- Pointer moves add into the unacked totals as they arrive.
- The phone emits the latest totals on a short timer (aim for 10 ms while a finger is down). It does not queue a datagram per event.
- A finger-up click is a TCP control message, not a bit on the motion packet.

If a real device cannot hold that rate, lower the timer. Do not rewrite the UI to chase it.

## 6. What we are not building in the first slice

The brief's roadmap puts a thin slice first: one phone platform, one desktop platform, movement and click. That slice is **Android + Windows**. iOS and macOS are the next platforms, not a rewrite. Keyboard, media keys, presentation mode, clipboard, and control from outside the LAN are later products. They do not get a packet type until an ADR says so.

## 7. Team fit

Nissi owns the Flutter trackpad surface. Allan owns the desktop agent, the input adapters, and the protocol oracle. Bubu owns latency and the "does it feel like a trackpad" gate. Virgil owns the touch surface: how much of the phone is pad, where the status sits, and what a disconnected state looks like. Outside contributors can change a gesture without touching `SendInput`.
