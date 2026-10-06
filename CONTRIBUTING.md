# Contributing

This repository is public. Pull requests are the way in, including from people who are not on the KAVD team.

## KAVD team

| Person | Primary | GitHub |
|---|---|---|
| Bubu (Daniel Mawuena) | CTO, QA, latency | `danbubu` |
| Nissi | Phone UI | `kkopong` |
| Allan | Desktop agent, protocol | `allan-osei` |
| Virgil | Touch surface | `Nana-asante-nocturnal` |

## Local setup

- Node.js 22 or newer (`npm test`)
- Flutter stable, once you work on `apps/phone`
- Rust stable and the Tauri prerequisites, once you work on `apps/desktop`
- Windows for the first input adapter. macOS for the second. Neither replaces the other.

```bash
npm test
```

## Branching

`main` stays buildable. Branch `feat/…`, `fix/…`, `docs/…`. One phase from `docs/PROMPTS.md` per PR. Your first session is the block with your name in `docs/KICKOFF_PROMPTS.md`. Squash-merge with a Conventional Commit title.

## Rules that keep the project open

- MIT. Do not add a CLA.
- Do not commit a kernel driver, a hidden input helper, or a cloud account requirement for local use.
- Protocol changes update `docs/PROTOCOL.md` and the tests together.
- UI copy must not claim the phone can control a computer that has not been paired.

## Definition of done

`npm test` green. A protocol change has a failing test that the change makes pass. A gesture change names which part of the brief it implements, and what it refuses to do (keyboard, off-LAN, a second monitor special case).
