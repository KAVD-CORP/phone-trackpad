# ADR-0003 — Cumulative UDP motion, reliable clicks

Status: Accepted (6 Oct 2026)

Relative motion is not a gamepad snapshot. Dropping a datagram would drop part of the swipe. Sending every move on TCP would let one late packet stall the cursor.

Each UDP motion packet carries the int32 totals since the session started. The desktop applies the gap from the last accepted totals. Sequence decides order. Clicks, button up and down, and scroll are TCP control messages because losing one is user-visible.

The brief's `MOVE dx=12` line is a sketch. The wire format is `docs/PROTOCOL.md`. Absolute screen coordinates are rejected.
