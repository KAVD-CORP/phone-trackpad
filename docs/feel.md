# Pointer feel (Phase 2 defaults + how to tune)

Tuning status: defaults chosen from conservative desktop-trackpad values;
device tuning (iPhone 15, 60/120 Hz) is a Phase 5 task with a real phone.
Loopback latency is 0.3–0.5 ms, so feel is dominated by the gain curve and
thresholds below, not the network.

## Gain curve (`apps/mobile/lib/gesture.dart`)

`pointerGain(speed, sensitivity, accel)`:
- accel off → flat `sensitivity`.
- accel on → `0.7x` below 150 px/s (pixel precision), linear ramp to `2.0x`
  at 3000 px/s, clamped above. Direction is preserved; gain applies to the
  movement magnitude with sub-pixel remainders carried forward.

## Thresholds (same file, exposed constants)

| Constant | Value | Meaning |
|---|---|---|
| `tapMaxMs` / `tapMaxPx` | 200 ms / 12 px | tap vs press-drag |
| `doubleTapWindowMs` / `doubleTapMaxPx` | 350 ms / 24 px | tap-hold-drag anchor |
| `dragHoldMs` / `dragSlopPx` | 250 ms / 10 px | hold still to grab |
| `accelPrecisionSpeedPxS` / `accelFullSpeedPxS` | 150 / 3000 | curve knees |

## Desktop side

- 8 ms injection tick (`TICK_MS`); MOVE/SCROLL summed per tick.
- Scroll: 40 finger px per wheel notch (`SCROLL_NOTCH_PX`); natural toggle
  applied on the phone before sending.
- Sensitivity range 0.2–3.0 in the phone settings sheet (in-memory; persisted
  storage is Phase 5).

## How to tune (Phase 5)

1. Run the harness on 5 GHz Wi-Fi; confirm p95 RTT/2 < 15 ms first — if the
   network is the problem, no curve fixes it.
2. Precision test: slowly trace a small circle; lower `accelLowFactor` if
   it jumps, raise if it lags.
3. Traversal test: one fast fling should cross a 1080p screen; raise
   `accelHighFactor` if it falls short.
4. Record final constants + phone model in this file.
