# Protocol

Little-endian. Version 1. Three messages. No JSON on the pointer path.

Ports: UDP **4620** phone to desktop (motion), UDP **4621** desktop to phone (ack), TCP **4622** (pairing and control).

## Motion — 20 bytes — magic `TP`

| Offset | Type | Field |
|---|---|---|
| 0 | u8 | `T` (0x54) |
| 1 | u8 | `P` (0x50) |
| 2 | u8 | version = 1 |
| 3 | u8 | flags, must be 0 in v1 |
| 4 | u32 | session, never 0 |
| 8 | u32 | sequence |
| 12 | i32 | dx total since session start |
| 16 | i32 | dy total since session start |

`dx` and `dy` are **totals**, not the delta since the previous datagram. The desktop subtracts the last accepted totals with int32 wrap. A lost packet is contained in the next total. A packet whose sequence is not newer is ignored.

Golden vector (session `0x01020304`, seq 7, dx 12, dy -4):

```
54 50 01 00 04 03 02 01 07 00 00 00 0c 00 00 00 fc ff ff ff
```

Sensitivity is applied on the phone before it adds into the total.

## Ack — 12 bytes — magic `TA`

| Offset | Type | Field |
|---|---|---|
| 0 | u8 | `T` |
| 1 | u8 | `A` (0x41) |
| 2 | u8 | version = 1 |
| 3 | u8 | reserved 0 |
| 4 | u32 | session |
| 8 | u32 | highest motion sequence applied |

The phone may subtract acknowledged totals from what it sends next. The desktop's rule does not depend on that; it only needs monotonic totals.

## Control — 18 bytes — magic `TC` — TCP only

| Offset | Type | Field |
|---|---|---|
| 0 | u8 | `T` |
| 1 | u8 | `C` (0x43) |
| 2 | u8 | version = 1 |
| 3 | u8 | kind |
| 4 | u32 | session |
| 8 | u32 | sequence |
| 12 | u8 | button (0 left, 1 right, 2 middle) |
| 13 | u8 | reserved 0 |
| 14 | i16 | scroll dx, else 0 |
| 16 | i16 | scroll dy, else 0 |

| Kind | Value | Meaning |
|---|---|---|
| LeftClick | 1 | one left click |
| RightClick | 2 | one right click |
| ButtonDown | 3 | button from byte 12 goes down (drag) |
| ButtonUp | 4 | button goes up |
| Scroll | 5 | wheel by the i16 pair. Natural scrolling is already applied. |

Unknown kind: drop. Do not invent a keyboard kind here.

## Versioning

A new field is a version bump and an ADR. Do not append a quiet byte.
