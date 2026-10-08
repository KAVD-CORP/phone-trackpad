# Packaging, signing & store notes (Phase 5)

## Windows installer (verified locally)

```powershell
cd apps/desktop
npx tauri build   # NSIS installer -> src-tauri/target/release/bundle/nsis/
```

`tauri build` downloads NSIS automatically when missing. The installer
bundles a WebView2 bootstrapper (downloaded on the user's machine at
install time), so no WebView2 work is needed. Ports: the installer must
not pre-open firewall rules silently — first app start triggers the
Windows Firewall prompt for private networks (UDP 51515, TCP 51516),
which the setup guide (`docs/setup.md`) walks through.

## Signing the Windows build (you must do this yourself)

Unsigned installers show a SmartScreen warning. Options:

1. **Azure Trusted Signing** (recommended): Microsoft's hosted signing
   service; `signtool` with the Trusted Signing endpoint after
   `tauri build`, or sign in CI with the `azure/trusted-signing-action`.
2. **EV code-signing certificate** (USB token or HSM): classic SmartScreen
   reputation path; `signtool sign /fd SHA256 /a <installer>`.
3. **Self-signed cert**: removes nothing for other users, but lets your own
   machines install quietly. Never ship this to others.

Unsigned local testing is fine: Windows runs the `.exe` after one
"Unknown publisher" confirmation.

## macOS (deferred — no Mac in use)

When the MacBook returns: `npx tauri build --bundles dmg` on the Mac,
then sign + notarize (requires an Apple Developer account, which we don't
have yet):

```bash
codesign --deep --force --sign "Developer ID Application: ..." target/release/bundle/macos/*.app
xcrun notarytool submit <dmg> --apple-id ... --team-id ... --wait
xcrun stapler staple <dmg>
```

Without notarization, macOS Gatekeeper blocks the app on other machines.
Also needed on first run: Accessibility permission onboarding (already in
the product spec; the macOS injector + `AXIsProcessTrusted` flow is the
remaining engineering when macOS returns).

## Phone store builds (notes, not yet run)

- **Android:** `cd apps/mobile && flutter build apk --release`
  (needs the Android SDK + NDK for the Rust `.so`; FRB docs cover the
  `cargo-ndk` wiring — budgeted for the store-build pass).
- **iOS:** needs a Mac with Xcode + paid Apple Developer account:
  `flutter build ipa`. Until then, sideload via the MacBook with a free
  Apple ID (7-day certificate) for device testing.
- Permissions to keep declared: local network + Bonjour (iOS), multicast
  + camera (Android). Camera is only used for the QR scan screen.

## Battery method (phone; measure on device)

Design already minimizes drain: no polling loops (50 ms hold-timer only
while touching), discovery stops off the discovery screen, zero packets
when idle, screen-keep-awake only on the trackpad screen. To measure:
fully charge, 30 min active use vs 30 min idle-connected, compare
Settings → Battery per-app %. Target: idle-connected indistinguishable
from background; active use dominated by screen, not our socket.
