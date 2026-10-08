# Mobile app (Phase 4)

Flutter `3.35.4` / Dart `>=3.9.0`. Pinned in root `.flutter-version`.
Screens: onboarding/discovery, QR scan, short-code entry, trackpad,
settings. Crypto runs in Rust via flutter_rust_bridge (same core).

## First-time setup (generates ios/ + android/ folders)

The repo tracks `lib/`, `test/`, `pubspec.yaml` only. Run once:

```powershell
cd apps/mobile
flutter create --platforms=ios,android --project-name trackpad_mobile .
flutter pub get
```

## Regenerate the Rust bindings (after touching `crates/trackpad-bridge`)

```powershell
# from the repo root
flutter_rust_bridge_codegen generate   # pinned 2.13.0, see flutter_rust_bridge.yaml
```

CI fails if the generated files (`lib/src/rust/`,
`crates/trackpad-bridge/src/frb_generated.rs`) are stale.

## iOS / Android keys (add after flutter create)

`ios/Runner/Info.plist` additions: `NSLocalNetworkUsageDescription`,
`NSBonjourServices: [_wptrackpad._tcp]`, `NSCameraUsageDescription`.
`android/.../AndroidManifest.xml`: `CHANGE_WIFI_MULTICAST_STATE`, `CAMERA`.

## Run

```powershell
cd apps/mobile
flutter analyze
flutter test   # gesture + widget + FFI smoke (needs target/debug/trackpad_bridge.dll on PATH)
flutter run   # iPhone + Windows host on the same Wi-Fi; scan the desktop QR
```

Keep the phone awake on the trackpad screen. No input is sent while idle.
