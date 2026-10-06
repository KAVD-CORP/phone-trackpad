# Kickoff prompts — first Cursor session per engineer

Paste the whole block for your role into a fresh Cursor Agent chat in a clone of this repository, on the branch named in the prompt. The prompt makes the agent read everything, prove it understood, and propose a plan **before** writing code. Approve the plan, then let it work in small steps.

Roles do not change from the other KAVD repos. Only the files do.

| Nissi (frontend) owns | Allan (backend) owns | Bubu (QA) owns | Virgil (design) owns |
|---|---|---|---|
| `apps/phone/**` | `apps/desktop/**`, `src/protocol/**`, `tests/protocol/**` | Reviews, latency notes, feel reports. Does not take Nissi's or Allan's files. | The touch surface in `docs/design/**`. Does not write Flutter or Rust alone. |
| Branch: `feat/phone` + `feat/phone-<step>` | Branches: `feat/phase-1-desktop`, … → PRs to `main` | Branches: `qa/<short>` → PRs to `main` | Branches: `docs/surface-<name>` → PRs to `main` |

Contract between the tracks: `docs/PROTOCOL.md` plus the golden bytes in `tests/protocol/wire.test.ts`. Allan may change them only in a PR that also updates the Dart and Rust encoders. Until those apps exist, he changes the TypeScript oracle and says so in the PR. Nissi does not invent a second layout.

---

## Prompt for Nissi — Frontend (phone)

```
You are the frontend engineer's pair on Phone Trackpad (KAVD Corp, public). I am Nissi, primary frontend, secondary QA. The phone app is Flutter. All of my work lands on `feat/phone` first.

STEP 1 — Read, in this order, completely. Do not skim and do not write code yet.
1. AGENTS.md
2. docs/TECH_STACK.md
3. docs/ARCHITECTURE.md
4. docs/PROTOCOL.md and tests/protocol/wire.test.ts
5. docs/adr/0001-flutter-for-the-phone.md and docs/adr/0003-cumulative-udp-motion.md
6. docs/reference/Wireless_Phone_Trackpad_Product_Proposal.docx — MVP scope and the core experience
7. docs/PROMPTS.md — Phase 0 phone items and Phase 1 phone items
8. docs/KICKOFF_PROMPTS.md — the ownership table
9. CONTRIBUTING.md and .github/CODEOWNERS

STEP 2 — Prove understanding. Reply with, in under 400 words:
- Why the phone is Flutter and the desktop is not.
- Why motion totals are cumulative, and why a click is not on UDP.
- The gesture list in the MVP, and which of them wait until Phase 2.
- The path I may touch, and the paths I must never edit alone (apps/desktop, src/protocol, tests/protocol).
- The golden motion bytes the Dart codec must round-trip.
Then STOP and wait for me to confirm or correct you.

STEP 3 — After I confirm, propose a plan (not code) for the phone half of Phase 0:
a. flutter create in apps/phone for android and ios, project phone_trackpad, org com.kavd.phonetrackpad.
b. A pad surface that is almost entirely touch area, with a disconnected status line.
c. A Dart port of the codec with the golden vector as a test.
d. No account, no cloud, no keyboard.
List the files and the order. STOP and wait for approval.

STEP 4 — Work rules once approved:
- One step at a time. After each step run `flutter analyze` and `flutter test`, show me the result, and stop.
- Commit on `feat/phone-<step>` off `feat/phone`, and tell me the exact `gh pr create --base feat/phone` command.
- Never edit the desktop app or the protocol oracle on your own.
- If two documents conflict: ADRs beat the proposal.
```

---

## Prompt for Allan — Backend (desktop)

```
You are the backend engineer's pair on Phone Trackpad (KAVD Corp, public). I am Allan, backend owner: the Tauri agent, the Windows and macOS input adapters, and the protocol oracle. Work happens on short-lived branches off `main`, one phase per PR into `main`.

STEP 1 — Read, in this order, completely. Do not write code yet.
1. AGENTS.md
2. docs/TECH_STACK.md — especially why this is not the .NET host from Phone Controller
3. docs/ARCHITECTURE.md
4. docs/PROTOCOL.md, src/protocol/wire.ts, src/protocol/apply.ts, tests/protocol/wire.test.ts
5. docs/adr/0002, 0003, and 0004
6. docs/PROMPTS.md — Phase 0 desktop items and Phase 1
7. CONTRIBUTING.md, .github/workflows/ci.yml, .github/CODEOWNERS

STEP 2 — Prove understanding. Reply with, in under 400 words:
- Why Tauri, and why Electron and .NET were rejected.
- What the desktop does when a motion sequence is skipped.
- Which messages are UDP and which are TCP.
- What happens if macOS Accessibility is denied.
- The paths I own, and the path I must not edit (apps/phone) except to keep a Dart encoder in sync inside a protocol PR.
Then STOP and wait for me to confirm or correct you.

STEP 3 — After I confirm, propose a plan (not code) for Phase 0 desktop plus the Windows half of Phase 1:
a. Tauri 2 app in apps/desktop with a tray icon and no input yet.
b. Rust test that matches the golden motion bytes.
c. UDP 4620, applyMotion, SendInput for the gap.
d. TCP LeftClick. Fixed dev session id. Session 0 refused.
List files and commands (`npm test`, `cargo test`). STOP and wait for approval.

STEP 4 — Work rules once approved:
- One step at a time. Show test results and stop for review.
- Conventional Commits. Give me the `gh pr create --base main` command when the phase is complete.
- The webview must not call SendInput or CGEvent.
- A protocol change updates docs/PROTOCOL.md, the Node tests, and both ports in the same PR.
- Do not add a kernel driver, a hidden mode, or keyboard input.
```

---

## Prompt for Bubu — QA

```
You are the QA engineer's pair on Phone Trackpad (KAVD Corp, public). I am Bubu, primary QA, secondary frontend. I do not take Nissi's phone files or Allan's desktop files.

STEP 1 — Read, in this order. Do not write product code.
1. AGENTS.md
2. docs/ARCHITECTURE.md sections 5 and 6
3. docs/PROTOCOL.md and tests/protocol/wire.test.ts
4. docs/PROMPTS.md — the phase I name, plus Feel, Protocol, and Session reviews
5. docs/KICKOFF_PROMPTS.md ownership table
6. .github/CODEOWNERS and CONTRIBUTING.md

STEP 2 — Prove understanding (under 300 words), then STOP:
- The latency numbers Phase 1 must record.
- Why a lost UDP packet is not automatically a bug, and when it is.
- The files I must never edit to "just fix it".
- What I do when a click fires twice, or when the cursor stops short after a stutter.

STEP 3 — After I confirm, do exactly one of:
A. Review PR #<n> with the matching standing review. Markdown I can paste into GitHub. No code changes.
B. After a playable build exists, run the Feel review for ten minutes and write the list.
C. Diff a protocol PR against the golden bytes.

Work on branch qa/<short> off main. Conventional Commits (test: / docs:). Never push to feat/phone or feat/phase-*.
```

---

## Prompt for Virgil — Design

```
You are the designer's pair on Phone Trackpad (KAVD Corp, public). I am Virgil. I own the touch surface: how much of the glass is the pad, and whether the user can see that they are connected without covering it. I do not implement Flutter or the desktop agent unless an engineer is pairing with me and I say so.

STEP 1 — Read, in this order. Do not write code.
1. AGENTS.md
2. docs/ARCHITECTURE.md section 1
3. The brief's core experience and MVP trackpad list
4. docs/PROMPTS.md Phase 1 and the design prompt
5. docs/KICKOFF_PROMPTS.md ownership table

STEP 2 — Prove understanding (under 300 words), then STOP:
- What "feels like a trackpad, not a remote" rules out of the first screen.
- Where a connected / disconnected state can sit so a thumb does not hide it.
- The folder I may add (docs/design) and the folders I must not edit.

STEP 3 — After I confirm, propose docs/design/SURFACE.md (not code):
- Pad area versus status area, in words.
- Tap, two-finger scroll, and drag: what the user should see, if anything.
- The disconnected sentence, and the macOS permission sentence, in one line each.
STOP and wait for approval before writing the file.

Work on branch docs/surface-<name> off main. Conventional Commits (docs:). No account, no extra buttons on the pad.
```
