# ADR-0005 — Handsfree keyboard on the existing injector

Status: Accepted (28 Oct 2026)

Keyboard text and key combos were out of scope until an ADR added them. Handsfree needs both: Muse Spark types into a form, and the person can still press Enter or Ctrl+C from the phone.

This does not add a driver. Windows keeps using `SendInput`. macOS will use `CGEvent` when that injector exists. Text is Unicode key events. A combo is modifiers plus one key. Media keys stay out. Control from outside the LAN stays out.

These actions are reliable, so they follow the click rule: they are not UDP motion. A lost motion packet must still be caught up by the next total, and a typed character must not be. The phone sends them as a Handsfree TCP frame on the existing pairing port, sealed with a key derived from the paired session. Session 0 is rejected. Revoking the phone makes the next frame a no-op.

The model loop runs in the desktop service, not in the webview. It calls the same `InputInjector` the trackpad uses. Relative motion stays relative. Absolute screen coordinates stay off the wire. A screenshot tool and a `Segmenter` trait are the seam for SAM 3.1; this ADR does not turn segmentation on.
