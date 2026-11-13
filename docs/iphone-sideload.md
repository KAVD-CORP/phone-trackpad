# iPhone development and testing

The phone app is the existing Flutter application in `apps/mobile`. Its iOS
project is committed: **do not run `flutter create .` over it**. The Windows
companion remains the test host; a macOS desktop adapter is a separate milestone.

For completed build and simulator results, see [validation status](ios-validation.md).

## Mac prerequisites

Use Flutter **3.35.4** (bundled Dart **3.9.2**), Rust **1.90.0**, and the
existing FRB **2.13.0** bindings. Do not run `flutter upgrade` or `cargo update`.
Node is pinned to **22.17.1** in `.nvmrc`; the protocol tests accept Node >=22.

1. Install Xcode from the Mac App Store. Open it, accept its license, and install
   the iOS platform and an iOS simulator runtime in Xcode Settings → Components.
   Command Line Tools alone do not contain the iOS SDK or simulator.
2. In Terminal, run the following. `sudo` asks for your Mac password locally;
   never paste that password into a chat.

   ```bash
   sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
   sudo xcodebuild -runFirstLaunch
   xcodebuild -showsdks
   xcrun simctl list devices available
   ```

3. Install Flutter 3.35.4 from the [SDK archive](https://docs.flutter.dev/install/archive),
   choosing **arm64** for Apple Silicon. Alternatively, clone the exact tag:

   ```bash
   git clone --depth 1 --branch 3.35.4 https://github.com/flutter/flutter.git ~/flutter
   export PATH="$HOME/flutter/bin:$HOME/.cargo/bin:$PATH"
   ```

4. Install Rust through [rustup](https://rustup.rs), then, from the repository root:

   ```bash
   rustup toolchain install 1.90.0 --profile minimal --component rustfmt,clippy
   rustup target add --toolchain 1.90.0 aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
   brew install cocoapods
   flutter doctor -v
   ```

   The three targets cover iPhone, Apple Silicon simulator, and Intel simulator.
   CocoaPods integrates `mobile_scanner` and `flutter_secure_storage_darwin`.
   The app and Pods target iOS **15.0** because Xcode 27 rejects older
   deployment targets. Plugin versions remain pinned.

### Optional workspace-local toolchain

If the pinned toolchain is installed in a sibling `.tooling` directory, source
its environment file before running these commands:

```bash
source ../.tooling/env.sh
```

CocoaPods is installed through Homebrew. When opening Xcode from Finder, it may
not inherit `CARGO_HOME` and `RUSTUP_HOME`. Prefer `flutter run` from this terminal.
For interactive Xcode builds with this local installation, quit Xcode first and
launch its executable from the sourced terminal:

```bash
/Applications/Xcode.app/Contents/MacOS/Xcode apps/mobile/ios/Runner.xcworkspace
```

## Fetch dependencies and run host checks

From the repository root:

```bash
npm test
cargo test --locked -p trackpad-core -p trackpad-bridge
cargo clippy --locked -p trackpad-core -p trackpad-bridge --all-targets -- -D warnings
cargo build --locked -p trackpad-bridge
cd apps/mobile
flutter pub get
flutter analyze
FRB_DART_LOAD_EXTERNAL_LIBRARY_NATIVE_LIB_DIR="$PWD/../../target/debug" flutter test
```

The environment variable tells FRB 2.13.0 where the workspace's **host** dynamic
library lives. A macOS host FFI test does not validate the iOS static library.
Keep `Cargo.lock` and `pubspec.lock`; dependency resolution is not an upgrade step.

## Build for the simulator

```bash
cd apps/mobile # from repository root
flutter pub get
flutter build ios --simulator --debug
xcrun simctl list devices available
# The iPhone 18 Pro simulator was used for the recorded validation.
xcrun simctl install <simulator-id> build/ios/iphonesimulator/Runner.app
xcrun simctl launch <simulator-id> com.example.trackpadMobile
```

With Xcode 27 on this Mac, `flutter run -d <simulator-id>` currently rejects the
named simulator destination before launching. Use `simctl` as above after a
successful generic simulator build.

Flutter invokes `pod install` as part of the iOS build. To run it explicitly:
`cd apps/mobile/ios && pod install`. Open `Runner.xcworkspace`, not the `.xcodeproj`.
`ios/Podfile.lock` is included in these branch changes to preserve the resolved native plugin set.
The production simulator build and on-simulator integration test passed on this Mac. To
repeat the native integration test with the current Xcode destination issue, build
its entry point and launch it with `simctl`:

```bash
flutter build ios --simulator --debug --no-pub --target integration_test/ios_startup_test.dart
xcrun simctl install <simulator-id> build/ios/iphonesimulator/Runner.app
xcrun simctl launch <simulator-id> com.example.trackpadMobile
xcrun simctl spawn <simulator-id> log show --last 3m --style compact --predicate 'process == "Runner"'
```

Look for the integration test name and `All tests passed!` in the Runner log.
The simulator checks Rust startup, UI and a Keychain round trip, but cannot
replace actual iPhone camera, local network and touch testing.

## Connect and sign an iPhone

1. Connect an unlocked iPhone by USB. Accept **Trust This Computer** on the phone.
2. Open Xcode → Settings → Accounts and sign in with your Apple Account.
3. Open `apps/mobile/ios/Runner.xcworkspace`. Select the Runner project, then the
   **Runner target → Signing & Capabilities**. Enable automatic signing and select
   your Team (a Personal Team works for local testing). For command-line builds,
   keep the Team in ignored `apps/mobile/ios/Flutter/Signing.local.xcconfig` as
   `DEVELOPMENT_TEAM = YOUR_TEAM_ID`; the three Runner configurations include it.
   Choose a unique bundle identifier if the default one cannot be registered.
   Do not commit personal Team IDs or provisioning files.
4. Select the connected phone as the run destination. Let Xcode prepare it.
5. On iOS 16+, enable **Settings → Privacy & Security → Developer Mode**, restart,
   and confirm. If the option is absent, first pair the phone in Xcode's
   Window → Devices and Simulators. Trust the developer under Settings → General
   → VPN & Device Management if the phone requests it.
6. For the tested path on this Mac, build a **signed Release** app from the
   sourced terminal, then install and launch it with `devicectl`:

   ```bash
   cd apps/mobile # from repository root
   flutter build ios --release --no-pub
   xcrun devicectl device install app --device <iphone-id> build/ios/iphoneos/Runner.app
   xcrun devicectl device process launch --device <iphone-id> <your-bundle-identifier>
   ```

   The repository's default bundle identifier is `com.example.trackpadMobile`. On
   12 October 2026, this path displayed “Find your computer” on an iPhone 15
   and survived backgrounding and reopening. With pinned Flutter 3.35.4 and
   Xcode 27, a physical **Debug** run still stops in Flutter's
   `NOTIFY_DEBUGGER_ABOUT_RX_PAGES` LLDB breakpoint; use Release for the
   current device validation. See [the validation report](ios-validation.md).

Free Personal Team provisioning is temporary; rebuild when it expires (normally
seven days). Paid membership is for App Store/TestFlight distribution, not a
prerequisite for testing your own device. See Apple's current signing guidance in
Xcode if account limits or provisioning errors occur.

## What the iOS integration does

- Xcode's **Build Rust** phase calls `ios/scripts/build_rust.sh` before linking.
  Cargo reads `rust-toolchain.toml`, builds with `--locked`, selects the target
  from `PLATFORM_NAME` and `ARCHS`, and uses debug or release to match Xcode.
  The Flutter simulator app builds the current Mac architecture to avoid an
  Xcode 27 multi-architecture `lipo -verify_arch` incompatibility. The Rust script
  also supports combining simulator slices when invoked independently.
- The script gives host Cargo build scripts the macOS SDK; Rust resolves the
  iOS SDK from the target triple. Inheriting Xcode's iOS SDK for host compilation
  caused `ld: library System not found`.
- Rust outputs live in Xcode's target temporary directory. A static archive is
  force-loaded into Runner. Runner target settings disable dead stripping and
  keep global symbols visible; the linker exports them for FRB's process
  loader. The device executable is checked for FRB exports in CI.
- iOS 27 requires a UIKit scene lifecycle. Runner hosts one explicit Flutter
  engine in `SceneDelegate` and registers plugins before attaching its view.
- On iOS, `RustLib.init` explicitly loads process symbols. This is safe here
  because this application has exactly **one** FRB library. Revisit this if adding
  another FRB-based plugin. Generated bindings remain unchanged.
- `Runner.entitlements` limits Keychain access to the signed app's own identifier.
  Debug, Profile and Release inherit it. No App Group or shared credentials are used.
- `Info.plist` declares Camera, Local Network and `_wptrackpad._tcp`. iOS discovery
  uses `NetServiceBrowser`, avoiding raw multicast sockets and the restricted
  multicast entitlement. Other platforms retain their existing Dart discovery.
- Bonjour results carry the existing IPv4 address, ports and fingerprint. An
  invalid fingerprint cannot be used for short-code pairing; QR remains available.

## iPhone + Windows end-to-end checklist

Record phone/iOS version, Windows build/commit, Wi-Fi conditions and pass/fail.
These are manual gates, not claims established by compilation.

1. Put both devices on the same non-guest Wi-Fi. Start the Windows companion;
   allow it on private networks (TCP 51516 and UDP 51515). Avoid client isolation.
2. Launch the phone app. A usable first screen demonstrates Rust initialization
   and initial Keychain access completed. Allow Local Network access.
3. Check discovery shows the Windows computer. Pair once using its short code and
   approve on Windows. Forget it and pair again by scanning a fresh QR; allow Camera.
4. Verify denying approval, a wrong short code and an expired QR grant no control.
5. Move one finger in every direction; test slow precision and a long swipe.
6. Tap once for one left click; two-finger tap for right click.
7. Scroll vertically and horizontally with two fingers; test natural-scroll setting.
8. Press/hold and drag a window; release and confirm the button is no longer held.
9. Adjust sensitivity and verify motion changes without changing click behavior.
10. Force-quit/relaunch: the trusted computer should remain in Keychain. Tap Connect
    and verify reconnection. Disconnect Wi-Fi/reconnect and check the displayed
    state and recovery; record failures rather than assuming automatic recovery.
11. Revoke trust on Windows and verify the phone can no longer control it.
12. Deny Camera or Local Network, then restore it in Settings → Apps → Trackpad
    Mobile. Verify QR/discovery recovery and repeat on congested Wi-Fi.

**Existing compatibility issues, unchanged by this iOS work:** ADR-0003 and AGENTS
require cumulative UDP totals and TCP controls. The live v2 app in `lib/net.dart`
uses encrypted i16 deltas and UDP clicks/scroll/buttons, matching the current core
and Windows implementation. The Node oracle checks the older v1 behavior only.
Also, the pairing/reconnect FRB APIs are marked `sync`, so waiting for desktop
approval can block Flutter's UI despite the Rust API comment claiming a worker
pool. These require coordinated follow-up; do not change just iOS's wire format.
`docs/ARCHITECTURE.md`'s `_phonetrackpad._tcp` reference is stale: live code uses
`_wptrackpad._tcp`, which the iOS declaration follows.

## References

- [Flutter iOS setup](https://docs.flutter.dev/platform-integration/ios/setup)
- [Apple Developer Mode](https://developer.apple.com/documentation/xcode/enabling-developer-mode-on-a-device)
- [Apple local network privacy](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy)
- [FRB 2.13.0 loading implementation](https://github.com/fzyzcjy/flutter_rust_bridge/blob/v2.13.0/frb_dart/lib/src/platform_types/_io.dart)
- [Secure storage 10.3.4 configuration](https://pub.dev/packages/flutter_secure_storage/versions/10.3.4)
