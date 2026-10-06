# Cursor Agent Prompts — Phone Trackpad

Paste one phase into a fresh Agent chat. Every prompt starts by reading `AGENTS.md`. This repo is public: do not add KAVD-only secrets, and do not inject input from an unpaired device.

The first session for each person is `docs/KICKOFF_PROMPTS.md`. Roles match the other KAVD repos: **@Frontend** Nissi (Bubu secondary), **@Backend** Allan, **@QA** Bubu (Nissi secondary), **@Design** Virgil.

## Phase 0 — Skeletons and ports (@Frontend + @Backend)

```
Read AGENTS.md, docs/TECH_STACK.md, docs/PROTOCOL.md and docs/adr/0001 and 0002.

Flutter and Rust are not in the repo yet.

Phone (apps/phone):
1. flutter create . --project-name phone_trackpad --org com.kavd.phonetrackpad --platforms android,ios on the stable channel.
2. A portrait-or-landscape pad surface that is mostly empty touch area, plus a disconnected status line. No networking yet.
3. A Dart port of src/protocol that round-trips the golden motion bytes in tests/protocol/wire.test.ts.

Desktop (apps/desktop):
4. Create a Tauri 2 app. System tray on Windows. No input injection yet.
5. A Rust port of the same golden bytes, as a unit test.

Do not add accounts, a cloud relay, keyboard events, or Electron. Run npm test, flutter test, and cargo test. Open a PR titled "Phase 0: skeletons and codec ports".
```

## Phase 1 — Movement and click (@Backend + @Frontend)

```
Read docs/ARCHITECTURE.md, docs/PROTOCOL.md, docs/adr/0003.

This slice is Android + Windows only. Leave iOS and macOS compiling, but do not block on them.

Desktop:
1. UDP 4620 decoder. applyMotion. SendInput for the gap. Ignore stale sequences.
2. TCP 4622 accepts a control message and performs LeftClick. Log sequence gaps.
3. Session id is a fixed dev value for this phase. Pairing UI waits until Phase 4. Refuse session 0.

Phone:
4. While a finger is down, add movement into totals and emit the latest totals about every 10 ms. Do not queue one datagram per event.
5. Finger up sends LeftClick on TCP when the finger did not drag. A small movement threshold separates a tap from a swipe. Document the threshold in the PR.

Exit: the Windows cursor follows the phone, and a tap clicks. Record p50/p95 finger-to-cursor on Wi-Fi in the PR. Run npm test and the app tests.
```

## Phase 2 — Core gestures (@Frontend + @Backend)

```
Read the MVP scope in the brief (trackpad section).

1. Two-finger right click (TCP RightClick).
2. Two-finger scroll (TCP Scroll). Natural scrolling is a phone setting that flips dy before encode.
3. Click-and-drag: ButtonDown, motion totals, ButtonUp.
4. Pointer sensitivity slider, applied on the phone before totals change.
5. Connected / disconnected status on both sides. If TCP drops, stop sending motion.

Do not add keyboard.
```

## Phase 3 — The other two platforms (@Backend + @Frontend)

```
Read docs/adr/0002.

1. macOS adapter: CGEvent. If Accessibility is missing, show the permission screen and inject nothing.
2. iOS pad uses the same Dart codec and the same gestures as Android.
3. Confirm relative motion crosses onto a second monitor without a special case.
4. Menu bar item on macOS, tray icon on Windows, both showing connected state.

PR: "Phase 3: iOS and macOS".
```

## Phase 4 — Discovery and pairing (@Backend + @Frontend)

```
Read docs/adr/0004 and docs/ARCHITECTURE.md section 4.

1. Desktop advertises _phonetrackpad._tcp and shows a QR plus a short code.
2. Phone lists nearby computers. Typed host is the fallback when multicast fails.
3. Pairing exchanges a non-zero session id. Unpaired packets do nothing.
4. Trusted-device list with revoke.
5. MAC on motion and control using the session secret. Add a test that a flipped bit is rejected. Do not ship a release build without this.
6. Automatic reconnection to a trusted device.

No account. No cloud.
```

## Phase 5 — Feel (@QA + @Frontend)

```
1. Tune the 10 ms emitter and the tap-versus-swipe threshold on one Android phone and one iPhone.
2. Battery note: CPU while a finger is down, and while the app is open but idle.
3. Congested Wi-Fi: confirm a loss does not leave the cursor short (the protocol test) and record the p95 the user actually feels.
4. Write the numbers into the PR. Do not add features from the brief's "Future Features" list.
```

## Standing reviews

### Feel review (@QA)

```
Use the pad for ten minutes: cursor, tap, right click, scroll, drag. List every moment it did not feel like a laptop trackpad, and why. Do not change code.
```

### Protocol review (@QA)

```
Diff the Dart and Rust encoders against docs/PROTOCOL.md and tests/protocol/wire.test.ts. List any byte that can disagree, and any click that can travel on UDP. Do not change code.
```

### Session review (@QA)

```
Confirm this PR drops session 0, drops an unpaired id, and does not move the cursor when macOS Accessibility is denied. Do not change code.
```

## Design prompts (@Design)

```
Read the brief's core experience and docs/ARCHITECTURE.md. Write or update docs/design/SURFACE.md: how much of the phone is the pad, where the connected state sits, what a tap looks like so the user does not cover the status, and what the disconnected screen says. Do not edit apps/phone or apps/desktop.
```
