# Phase 2 manual checklist (Windows 10 + iPhone 15, same Wi-Fi)

Automated first: `cargo test --workspace` (22+ tests), then this list.
Park the cursor on empty desktop before any test ending in a click/drag.

1. Service: `cargo run -p trackpad-desktop --bin trackpad-service`.
2. Right click: `cargo run -p fake-phone -- rightclick` on an empty desktop
   spot → context menu appears. Esc to dismiss.
3. Scroll: `cargo run -p fake-phone -- scroll` over a scrollable window →
   page moves down ~10 notches then back up.
4. Drag: `cargo run -p fake-phone -- drag` on empty desktop → selection
   rectangle stretches ~300 px right, releases cleanly.
5. Stuck input: `cargo run -p fake-phone -- stuck`, then watch: no UP is ever
   sent, yet the service releases the button after ~500 ms of silence
   (drag a window afterwards to confirm nothing is grabbed).
6. LAN latency: `cargo run -p latency-harness -- <pc-ip> --count 200`.
   Target 5 GHz Wi-Fi: p50 < 15 ms one-way (RTT/2), p95 < 30 ms.
7. iPhone (`flutter run`, same gestures as the gesture tests):
   - tap → left click; two-finger tap → right click.
   - two-finger drag → scroll (flip Natural scroll, direction reverses).
   - tap, press-hold, drag → moves the item; release drops it.
   - add a second finger mid-drag → drag ends, scroll takes over, no click.
   - kill the app mid-drag → desktop releases within ~500 ms (same as 5).
8. Settings sheet (tune icon): sensitivity slider visibly changes speed;
   accel off feels linear; tap-to-click off disables taps only.

Report: harness output (step 6) + pass/fail per step.
