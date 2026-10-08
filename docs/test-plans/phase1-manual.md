# Phase 1 manual checklist (Windows 10 + iPhone 15, same Wi-Fi)

Prereqs: PC firewall allows `trackpad-service` UDP (approve the prompt on
first run). Find the PC LAN address with `ipconfig` (e.g. `192.168.1.10`).

1. Start the service: `cargo run -p trackpad-desktop --bin trackpad-service`.
   Expect `listening on 0.0.0.0:51515`.
2. Loopback latency (on the PC): `cargo run -p latency-harness -- 127.0.0.1`.
   Record min/mean/p50/p95/max. Loopback should show p95 well under 1 ms.
3. No-phone cursor test (on the PC): park the cursor on empty desktop first
   (the script ends with one real left click).
   `cargo run -p fake-phone -- 127.0.0.1`.
   Cursor glides right ~600 px, then one left click. If nothing moves, check
   the service window for `SendInput failed` (UIPI block: run from a normal
   user session, not an elevated prompt, for store-app targets).
4. LAN latency: run the harness against the PC address from step 1.
   Target on 5 GHz Wi-Fi: p50 < 15 ms one-way (RTT/2), p95 < 30 ms.
5. iPhone: `cd apps/mobile && flutter run`, enter the PC address, Connect.
   Drag one finger: cursor follows. Quick tap: left click. Two fingers: must
   do nothing (landed in Phase 2).
6. Kill test: close the app mid-touch. Cursor must never keep moving (MOVE is
   relative per event; no motion state is held).

Report: harness output for steps 2 + 4, and pass/fail per step.
