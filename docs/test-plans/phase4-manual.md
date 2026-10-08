# Phase 4 manual checklist (Windows 10 + iPhone 15, same Wi-Fi)

Automated first: `cargo test --workspace` (49+ tests), `flutter test`
(21 tests incl. FFI smoke), `flutter analyze` clean.

## Desktop pairing UI (Tauri app or service console)

1. Open the desktop app → **Show pairing code**: QR renders, 6-digit code
   shows, expiry ~2 min. (`trackpad-service --pair` prints the same.)
2. iPhone: **Scan pairing code** → approve **Allow** on the computer →
   phone becomes a trackpad immediately.
3. Deny path: scan again, click **Deny** → phone shows failure, no trust
   stored (`list` shows nothing new).
4. Expiry: wait 2+ min, scan the old QR → pairing fails. Show a new code.
5. Wrong code ×3 → 30 s lockout (4th attempt rejected immediately).
6. Short code: pick the computer in **On this Wi-Fi**, type the 6 digits →
   same approval → trackpad.
7. Revoke: Trusted phones → **Revoke** mid-drag → cursor stops within a
   packet; `live` clears. Re-pair works after.
8. Reconnect: kill the phone app, reopen → **Connect** → no approval, no
   code. Toggle Wi-Fi off/on → reconnects within seconds, no prompt.

## No-phone equivalents (all verified in automation/CI too)

- `fake-phone pair --qr-file qr.txt` (service `--auto-approve`): cursor
  glides +600 px and clicks. Park the cursor on empty desktop first.
- `latency-harness --qr-file qr.txt --count 200`: encrypted RTT.
  Target 5 GHz Wi-Fi: p50 < 15 ms one-way (RTT/2), p95 < 30 ms.
- Plaintext while secure: `fake-phone 127.0.0.1` (v1 MOVE) → cursor must
  NOT move (dropped + counted).

## Network/hostile cases

9. Guest Wi-Fi (AP isolation): discovery list empty, QR scan still pairs
   (address rides in the code). Short-code-only needs discovery — falls
   back to QR with a clear message.
10. iPhone denies Local Network permission: app shows no computers and
    scans fail — guide to Settings (message names the exact toggle).
11. Firewall set to Public: pairing TCP times out; setup.md covers the fix.
12. Kill phone mid-drag (swipe up): desktop releases the button ≤ ~500 ms
    (silence timeout), same as Phase 2.

## iPhone specifics (needs your device; blocked without Apple sideload)

13. Install via MacBook + free Apple ID sideload (7-day cert) until store
    builds land in Phase 5. Camera permission prompt appears on first scan.
14. Keep the phone awake on the trackpad screen; input stops when
    backgrounded (by design — no background input).

Report: harness numbers + pass/fail per step.
