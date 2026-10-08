# Build Prompt: Wireless Phone Trackpad

> Paste this whole file into Muse Spark 1.3 as the first message (or save it as the project's `PROMPT.md` / context file). Work through the phases in order. Do not skip ahead.

---

## 0. Your Role

You are a senior engineer building **Wireless Phone Trackpad**: an iPhone/Android app that turns the phone into a wireless trackpad for a Windows or macOS computer, plus a lightweight desktop companion that receives input and injects it into the OS.

**Primary product principle:** it must feel like a real trackpad, not a remote-control utility. Cursor feel, latency, and effortless pairing matter more than feature count.

## 1. How to Work

1. **Phase by phase.** Complete one phase, show what you built, how to run it, and how you verified it. Then stop and wait for me to say "next".
2. **Thin vertical slice first.** Phase 1 is one phone platform + one desktop platform + move/click, with latency measured, before anything else.
3. **Plan before code.** At the start of each phase, give a short plan (files to create, key decisions, risks) before writing code.
4. **Real, runnable code.** No pseudocode, no `TODO: implement`, no placeholder functions pretending to work. If something cannot be done in this environment (real-device testing, code signing), say so and write a manual test checklist instead.
5. **Ask only when blocked.** Choose sensible defaults, state them in one line, and continue. If you hit a real fork, ask one concise question.
6. **Never invent APIs.** If unsure about a crate/package API or version, say so and check the docs rather than guessing. Pin dependency versions.
7. **Tests with the code.** Every protocol, crypto, and pairing change ships with tests.
8. **Keep me informed of tradeoffs** in 1–3 sentences, not essays.

## 2. Product Summary

**Flow:** install the desktop companion → open the phone app → computer is discovered automatically → pair by QR code (or short code) → phone is immediately a trackpad.

**No account, no cloud** for normal local use. Everything runs over the local network.

**Platforms:** iOS, Android, Windows 10/11, macOS (13+, Apple Silicon and Intel).

### MVP scope

**Trackpad**
- Smooth relative cursor movement
- Tap to click
- Two-finger tap = right click
- Two-finger scroll
- Click-and-drag (tap-then-hold-and-drag, plus optional drag lock)
- Adjustable pointer sensitivity
- Natural / inverted scrolling toggle

**Connectivity**
- Automatic LAN discovery
- QR or short-code pairing
- Automatic reconnection
- Clear connected/disconnected status on both phone and desktop
- Trusted-device list with revocation

**Desktop**
- Windows and macOS companions
- System tray / menu bar operation
- Start-on-login option
- Simple settings and device management

### Out of scope for the MVP (design so they can be added later)
Phone keyboard, media/volume controls, presentation/laser-pointer mode, custom shortcut buttons, clipboard sync, lock/sleep, multi-computer management, custom gesture mapping, remote access outside the LAN.

## 3. Technology Stack (use exactly this unless you can justify a change with evidence)

| Area | Choice | Why |
| --- | --- | --- |
| Monorepo | Single repo, Cargo workspace + Flutter app + Tauri app | One source of truth for the protocol |
| **Shared core** | **Rust crate `trackpad-core`**: protocol, framing, crypto, pairing state machine, replay protection | Same code runs on desktop and phone, so the two sides cannot drift apart |
| Mobile | **Flutter (Dart)** for iOS + Android | One codebase, excellent gesture/touch handling |
| Rust ↔ Dart | **flutter_rust_bridge** (FFI) | Phone uses the exact same `trackpad-core` as the desktop |
| Desktop shell | **Tauri 2** (Rust backend) | Small footprint, tray/menu-bar support, autostart plugin |
| Desktop UI | **TypeScript + React + Vite** (minimal) | Settings, pairing QR, device list |
| Input injection | **Native APIs behind a `trait InputInjector`** | Reliability and correct OS integration |
| Windows injector | `SendInput` via the `windows` crate | Official API |
| macOS injector | `CGEvent` via `core-graphics` / `objc2`, `AXIsProcessTrusted` for permission checks | Official API, proper Accessibility flow |
| Discovery | **mDNS / DNS-SD** (`mdns-sd` crate on desktop; Flutter side via the same Rust crate or `nsd`) | Zero-config LAN discovery |
| Pairing channel | **TCP**, secured with the **Noise protocol** (`snow` crate) | Reliable, authenticated, simple |
| Input channel | **UDP**, encrypted with a session key (ChaCha20-Poly1305 AEAD) | Lowest latency; newest movement wins |
| Short-code pairing | **SPAKE2** (`spake2` crate) | A 6-digit code is weak; a PAKE makes it safe |
| QR | `qr_flutter` (generate on desktop UI via a TS QR lib; scan on phone with `mobile_scanner`) | Mature packages |
| Local storage | Desktop: OS keychain (`keyring` crate) for keys, JSON for settings. Phone: `flutter_secure_storage` | Keys never in plain files |
| CI | GitHub Actions: Rust tests, Flutter tests, Tauri builds for Windows + macOS | Four-platform confidence |

**Rationale to keep in mind:** language choice is not the product. Latency, feel, security, and low friction are. The architecture must allow swapping any layer if measurements justify it.

## 4. Architecture

```
┌────────────────────┐        LAN        ┌──────────────────────────┐
│  Phone (Flutter)   │                   │ Desktop (Tauri 2)        │
│  Touch surface     │                   │ Tray + settings UI (TS)  │
│  Gesture engine    │   UDP: input      │ Service (Rust)           │
│  Settings, pairing │ ───────────────►  │  discovery, sessions     │
│        │           │   TCP: control    │        │                 │
│  trackpad-core ◄───┼───────────────────┼──► trackpad-core         │
│  (via FRB)         │                   │        │                 │
└────────────────────┘                   │  InputInjector (native)  │
                                         └──────────────────────────┘
```

**Four layers, strict boundaries:**

| Layer | Responsibility |
| --- | --- |
| Mobile UI | Touch surface, gesture recognition, settings, pairing UI |
| Protocol (`trackpad-core`) | Message types, encoding, encryption, sessions, pairing |
| Desktop service | Discovery advertising, networking, session management, coalescing |
| OS adapter | Cursor, buttons, scroll via native APIs only |

### Repo layout

```
wireless-phone-trackpad/
├─ crates/
│  ├─ trackpad-core/        # protocol, crypto, pairing, session (no I/O assumptions where possible)
│  ├─ trackpad-desktop/     # networking, discovery, trust store, injector impls
│  └─ trackpad-bridge/      # flutter_rust_bridge surface for mobile
├─ apps/
│  ├─ mobile/               # Flutter app
│  └─ desktop/              # Tauri 2 app (src-tauri + web UI)
├─ docs/                    # protocol spec, architecture, user docs, test plans
├─ tools/                   # latency harness, fake phone client, packet fuzzer
└─ .github/workflows/
```

## 5. Protocol Requirements

**Relative movement only** (never absolute coordinates), so behavior is independent of display resolution.

### Logical messages
`MOVE{dx,dy}`, `BUTTON_DOWN{button}`, `BUTTON_UP{button}`, `CLICK{button}`, `SCROLL{dx,dy}`, `PING/PONG{timestamp}`, `HELLO`, `BYE`.

### Wire format rules
- Compact binary (no JSON on the hot path). Version byte first.
- Per-packet: session id, monotonically increasing sequence number, message type, small payload, AEAD tag. Derive the nonce from the sequence number.
- **Replay and stale protection:** sliding replay window; reject duplicates and out-of-window packets.
- **MOVE and SCROLL are lossy and coalescible:** if packets are dropped or reordered, apply the newest, never "catch up" with a burst. Sum deltas that arrive within the same injection tick.
- **Button events are critical:** send redundantly (for example, repeat 2–3 times with the same sequence-keyed id) or acknowledge them, so a dropped `BUTTON_UP` can never leave the mouse stuck down. The desktop must also release any held button if the session ends or goes silent for N ms.
- Reject malformed or oversized packets without panicking. Fuzz this.
- Document the full wire spec in `docs/protocol.md` before implementing it.

### Sending rate
Phone coalesces touch deltas and sends at the display refresh cadence (60–120 Hz), with a capped packet rate and no sending when nothing moved. Sub-pixel remainders are carried forward, not dropped.

## 6. Pairing, Discovery, Security

Because this app can generate mouse input on someone's computer, **authentication is a core requirement, not a feature.**

### Discovery
- Desktop advertises `_wptrackpad._udp` / `_tcp` via mDNS with device name, protocol version, and port. No sensitive data in the advertisement.
- Phone lists discovered computers automatically.
- **Fallback when multicast is blocked** (guest Wi-Fi, AP isolation): the QR code carries the host IPs and port so pairing works without discovery. Also offer a manual "enter code + address" path in an "Advanced" screen, hidden from the normal flow.

### First-time pairing
1. Desktop shows a QR code with: desktop public key fingerprint, candidate addresses, port, and a **random 128-bit pairing secret that expires in about 2 minutes and works once**.
2. Phone scans and connects over TCP. Run a Noise handshake with the pairing secret as a pre-shared key. Both sides learn each other's long-term static public key.
3. Desktop shows "Allow *iPhone of Sam* to control this computer?" and requires explicit confirmation.
4. Both sides store the peer as trusted (keys in OS secure storage).
5. **No QR available path:** a 6-digit short code shown on the desktop, entered or confirmed on the phone, using SPAKE2 so the code cannot be brute-forced offline. Rate-limit attempts and lock out after repeated failures.

### Reconnection
- Known devices reconnect automatically with a Noise handshake using stored static keys, then derive a **fresh session key** for the UDP channel. No re-pairing, no user prompt.
- Reconnect within seconds after Wi-Fi blips, phone sleep/wake, and desktop sleep/wake. Backoff with jitter. Never spam the network.

### Security checklist (all required)
- Only paired devices can send input. Unauthenticated packets are dropped silently.
- Per-session keys; session expiry; replay protection.
- Trusted devices listed in desktop settings with a **Revoke** button that takes effect immediately and kills any live session.
- Visible "connected" indicator on both devices; desktop tray icon changes state while a phone is controlling.
- No cloud account, no telemetry containing personal data, no unnecessary data transmitted.
- Pairing mode is off unless the user opened the pairing screen.
- Write a short `docs/threat-model.md` (who can do what, what's protected, known limits).

## 7. Mobile App (Flutter)

### Screens
1. **Onboarding / find computer:** list of discovered computers, "Scan QR" button, plain-language status.
2. **Pairing:** QR scan or short-code entry, clear success/failure states.
3. **Trackpad (main):** a large full-bleed touch surface with minimal chrome. A slim status bar shows connection state and the computer's name. Settings reachable but unobtrusive.
4. **Settings:** pointer sensitivity, acceleration on/off, scroll direction (natural/inverted), tap-to-click on/off, haptics on/off, trusted computers.

### Gesture engine (treat as first-class engineering, with unit tests)
Implement as a pure, testable state machine separate from widgets, fed by raw pointer events:
- One-finger move → `MOVE` with a tuned pointer-acceleration curve (velocity-based, with a gentle low-speed region for precision) and sensitivity multiplier.
- One-finger tap → left click (tap timing and movement thresholds, tuned and exposed as constants).
- Two-finger tap → right click.
- Two-finger move → scroll (with inertia only if it can be done without adding latency; otherwise skip for the MVP).
- Tap-then-hold-and-drag → `BUTTON_DOWN`, moves, `BUTTON_UP` on release. Optional drag lock.
- Ignore palm/edge accidental touches reasonably. Handle multi-touch transitions cleanly (lifting one finger, adding a finger mid-gesture).
- Light haptic on click where supported.
- Keep the screen awake while on the trackpad screen. Release it otherwise.

### Platform specifics
- **iOS:** `NSLocalNetworkUsageDescription` and `NSBonjourServices` in `Info.plist`; camera usage string for QR; handle the local-network permission being denied with a helpful screen.
- **Android:** `CHANGE_WIFI_MULTICAST_STATE` and a multicast lock during discovery, camera permission, correct handling of Wi-Fi sleep behavior.
- Low battery use: no polling loops, stop discovery when not on the discovery screen, send nothing when idle.

## 8. Desktop Companion (Tauri 2)

- Runs from the **system tray (Windows) / menu bar (macOS)**; main window opens only for pairing/settings.
- Settings UI (React + TS): pairing QR + short code, trusted devices with revoke, "Start on login" toggle, current connection state, pointer settings if relevant, about/logs.
- `autostart` plugin for start-on-login.
- **Windows:** `SendInput` for move (relative), buttons, and wheel. Handle DPI scaling and multi-monitor correctly (relative moves should just work; verify). Installer must explain firewall prompts clearly; create the firewall rule correctly or guide the user through it.
- **macOS:** check `AXIsProcessTrusted` at launch and when pairing completes. First-run onboarding explains why Accessibility permission is needed with a button that opens the right System Settings pane, and re-checks automatically when the user returns. Never fail silently if permission is missing; show a clear state in the tray and the UI. Use correct `CGEvent` posting for move/drag/click/scroll (drag events use the dragged event types, not plain move).
- `InputInjector` trait with `WindowsInjector`, `MacInjector`, and a `MockInjector` for tests. Everything above the trait is OS-agnostic.
- Injection tick: apply coalesced deltas on a steady cadence. Do not block the network thread on OS calls.
- Safety: on session end, silence timeout, or revoke, **release all held buttons**.
- Structured logging (`tracing`) with a "copy diagnostics" button. Logs must not contain keys or secrets.

## 9. Performance Targets

Measure, don't assume. Build `tools/latency-harness` early.

- End-to-end phone-touch → cursor-move target: **p50 under 15 ms, p95 under 30 ms on normal 5 GHz Wi-Fi** (report what you actually measure and how).
- No allocation or heavy serialization on the hot path; reuse buffers.
- Idle CPU on desktop near zero; low CPU while active on both sides.
- Recover from short network interruptions without user action.
- Test under simulated loss/jitter (e.g., `tc netem` or a lossy UDP proxy in the harness).
- Include a "stuck input" test: kill the phone mid-drag and confirm the button is released.

## 10. Testing Requirements

- **`trackpad-core`:** unit tests for encode/decode round trips, replay window, nonce handling, handshake success/failure, expired and reused pairing secrets, malformed packets, plus `cargo-fuzz` or `proptest` for the decoder.
- **Pairing/reconnect:** automated end-to-end tests using the fake phone client against the desktop service with `MockInjector`.
- **Gesture engine:** table-driven tests from recorded pointer-event sequences (tap, double tap, two-finger tap, scroll, drag, finger added/removed mid-gesture).
- **Security:** unauthorized sender, stale session, replayed packets, revoked device, brute-forcing the short code.
- **Manual device checklists** in `docs/test-plans/` for: iPhone and Android of different screen sizes; Windows with 100/125/150% scaling and multi-monitor; macOS versions and Accessibility permission states (not granted, granted, revoked while running); Wi-Fi with AP isolation; congested Wi-Fi.
- CI runs all automated tests on every push.

## 11. Phases (stop after each one and wait for "next")

**Phase 0: Foundations.** Monorepo scaffold, toolchain versions pinned, CI skeleton, `docs/protocol.md` draft, `docs/architecture.md`. *Exit:* everything builds and runs "hello world" on at least the dev machines.

**Phase 1: Vertical slice.** `trackpad-core` basic MOVE/CLICK over UDP (no encryption yet, hard-coded address OK), Flutter touch surface sending moves/taps, Tauri/Rust desktop service with ONE real injector (pick the one for the OS I'm developing on, ask me which if unknown). *Exit:* I can move the cursor and click from a real phone; latency harness reports real numbers.

**Phase 2: Core trackpad.** Right click, two-finger scroll, drag, sensitivity, acceleration, natural scroll, stable versioned protocol, coalescing, stuck-button safety, reconnection basics. *Exit:* gesture-engine tests pass; feel is tuned and documented.

**Phase 3: Cross-platform.** Second desktop injector, both mobile OSes confirmed building and running. *Exit:* all four platform combinations work end to end.

**Phase 4: Discovery, pairing, security.** mDNS, QR + SPAKE2 short-code pairing, Noise handshakes, encrypted UDP, trusted devices, revoke, macOS Accessibility onboarding, firewall guidance, multicast-blocked fallback. *Exit:* security checklist in §6 passes with tests.

**Phase 5: Polish.** Latency tuning, gesture refinement, battery measurement, tray/menu-bar UX, start-on-login, error states and empty states, packaging (Windows installer, macOS dmg; note signing/notarization steps I must do myself), app-store build notes.

**Phase 6: Beta readiness.** Test plans executed or handed to me as checklists, user documentation (setup, troubleshooting: firewall, Accessibility, guest Wi-Fi), known-issues list.

**Phase 7 (future, design only):** Keyboard, media controls, presentation mode, shortcut buttons, multi-computer. Do not build. Only confirm the protocol and architecture have room for them (message-type registry, capability negotiation in `HELLO`).

## 12. Quality Bar and Rules

- Rust: `clippy` clean, `rustfmt`, no `unwrap()` on network/OS paths, errors typed (`thiserror`). Dart: `flutter analyze` clean, null-safe, state management kept simple (Riverpod or plain `ChangeNotifier`, pick one and stay consistent).
- Minimal dependencies; justify each non-obvious one in one line.
- Secrets and keys never logged, never in plain text files, never committed.
- UI copy is plain language for non-technical users. No mention of IP addresses, ports, or protocols in the normal flow.
- Accessibility basics on the mobile UI (labels, contrast, large targets) and honor system dark mode on both apps.
- Every phase ends with: **what works now, how to run it, how it was verified, known gaps, suggested next step.**

## 13. First Response

Do **not** write code yet. Reply with:

1. A restatement of the project in 5 lines or fewer, to confirm understanding.
2. Any assumptions you're making and the 1–3 questions you actually need answered (for example: which OS do I develop on, which phone do I have, do I have an Apple Developer account for iOS device testing).
3. Your Phase 0 plan: the file tree you will create and the exact toolchain versions you will pin.

Then wait for my go-ahead.
