# Phone Trackpad

Use your phone as a trackpad for a Windows or macOS computer. The hard part is not sending a touch. The hard part is a cursor that still feels continuous, with none of the setup a remote-control utility usually demands.

A **public** KAVD Corp repository. MIT license. Outside contributions are welcome.

## Start here

1. `AGENTS.md` — rules.
2. `docs/TECH_STACK.md` — Flutter on the phone, Tauri on the desktop, and the options we rejected.
3. `docs/ARCHITECTURE.md` — packet path, session, latency.
4. `docs/PROTOCOL.md` — the 20-byte motion total.
5. `docs/adr/` — Flutter, Tauri, UDP totals, pairing.
6. `docs/reference/Wireless_Phone_Trackpad_Product_Proposal.docx` — the brief.
7. `docs/KICKOFF_PROMPTS.md` — first Cursor session for Nissi, Allan, Bubu, and Virgil.
8. `docs/PROMPTS.md` — phase prompts for Cursor.

## What runs today

The wire format and its tests. The phone app and the desktop agent are Phase 0. Flutter and Rust are not vendored here.

```bash
npm test
```

## Layout

| Path | What |
|---|---|
| `src/protocol` | TypeScript codec and the "lost packet still moves the cursor" rule |
| `tests/protocol` | Golden bytes the Dart and Rust ports must match |
| `apps/desktop` | Tauri agent (created in Phase 0) |
| `apps/phone` | Flutter app (created in Phase 0) |

## Status

Stack decided. Phase 0 not started. First slice is Android + Windows, movement and click. iOS and macOS follow without a rewrite.
