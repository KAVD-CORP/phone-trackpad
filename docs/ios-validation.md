# iOS support validation — 11–12 October 2026

Branch: `codex/ios-support`, based on `715942c` (`origin/main` at the time of
review). This report records validation of the iOS changes before team review.

## Environment

- Apple Silicon (`arm64`), macOS 26.6.2 (25G83).
- Full Xcode **27.0 (27A266a)** selected at
  `/Applications/Xcode.app/Contents/Developer`; first-launch setup completed.
  The iOS 27 SDK, runtime and bootable iPhone 18 Pro simulator are installed.
- Workspace-local Flutter **3.35.4** / Dart **3.9.2**, Rust **1.90.0** with
  rustfmt/clippy and all three iOS targets. Source `../.tooling/env.sh` from the
  repository root. CocoaPods **1.17.0** is installed through Homebrew.
- `flutter doctor -v` recognizes Xcode and CocoaPods. Its Android SDK warning is
  outside this iOS milestone. The exact Flutter tag shows a channel warning.
- Node **22.20.0** / npm **10.9.3** is installed. It differs from `.nvmrc`
  (22.17.1) but satisfies the root package's >=22 requirement.
- On 11 October, `xcrun devicectl list devices` found no physical iPhone. An
  iPhone 15 running iOS 27.0.1 was connected for the 12 October follow-up.

## Physical iPhone follow-up — 12 October

Automatic signing remains enabled. The local development Team setting was moved
to an ignored `Signing.local.xcconfig` included by the Runner configurations;
the tracked project contains no personal Team ID. App entitlements remain enabled.

| Check | Observed result | Limit |
| --- | --- | --- |
| Signed Debug and Release builds with Flutter 3.35.4/Xcode 27 | **Passed** | Compilation and signing alone did not prove startup |
| Initial physical Debug launch | **Blank** while Flutter waited for LLDB/JIT handling | The CLI later stopped at `NOTIFY_DEBUGGER_ABOUT_RX_PAGES` breakpoint 1.1; Debug remains unresolved |
| Initial physical Release launch | **Blank** | Runner's effective `DEAD_CODE_STRIPPING=YES` removed all Rust FFI exports, despite `Rust.xcconfig` requesting `NO` |
| Corrected signed Release build | **Passed**, 20.1 MB app | `dyld_info -exports` and `nm -gU` now show `frb_get_rust_content_hash` and both FRB dispatchers in Runner |
| `devicectl` install and launch on iPhone 15 | **Passed** | Actual physical install and startup, without an attached debugger |
| iPhone screenshots | **“Find your computer” shown**; QR scanner screen and live camera preview also observed | No pairing code was scanned and no Windows connection was tested |
| Background and reopen | **Passed** | Opening Settings backgrounded the app; reopening showed “Find your computer” and the same Runner process remained alive |
| Simulator Debug rebuild after linker fix | **Passed** | `flutter build ios --simulator --debug --no-pub` still compiles |
| `npm test` after native fixes | **6 passed** | v1 protocol oracle regression check; Windows end-to-end remains separate |
| Final signed Release rebuild, reinstall and process launch after signing cleanup | **Passed** on 12 October; code signing verified, Rust FFI exports present, Runner process remained alive | The phone was locked during a new screen capture, which was black; visible startup is supported by the earlier unlocked Release screenshot, not that capture |

The final installed app is the corrected signed **Release** build. It was
reinstalled after the Debug diagnosis, launched, and showed “Find your
computer” on a fresh iPhone screenshot. The physical QR scanner screen showed
a live camera preview, but code recognition was not tested.

Two separate startup faults were fixed. `FlutterSceneDelegate` in pinned
Flutter 3.35.4 implements `scene(_:willConnectTo:options:)` but does **not**
implement the optional `sceneDidEnterBackground` or `sceneDidDisconnect`
selectors. Calling `super` for either throws `unrecognized selector`. Runner
now handles its Bonjour cleanup in those callbacks without forwarding them to
the superclass. Flutter's view controller observes scene notifications, and
its engine observes application or scene lifecycle as appropriate.

For Release, Runner target settings overrode `Rust.xcconfig` with dead-code
stripping enabled. The original signed Release executable exported only
`_main`; FRB's process loader could not find the Rust functions needed before
`runApp`. Runner now disables dead-code stripping in Debug, Profile and Release,
keeps global symbols visible, and links with `-export_dynamic`. A new CI check
asserts the required FRB exports in the unsigned device executable.

The reported Xcode warning that the `BreakpointLocations` capability was not
found was not reproduced in the Flutter CLI log, so its exact source and effect
remain unconfirmed. The generated Xcode scheme points to Flutter's LLDB init
file; a standalone LLDB load installed the `NOTIFY_DEBUGGER_ABOUT_RX_PAGES`
callback with auto-continue. In a verbose physical Debug run, Flutter attached
LLDB and connected to the Dart VM service, then stopped at that breakpoint
instead of continuing. This is a **separate Debug/JIT limitation** on this
pinned Flutter/Xcode/device combination; it did not block the signed Release
runtime test. No debugger workaround was claimed or applied to the app.

Windows protocol code and generated FRB bindings were unchanged. Real iPhone
pairing, Bonjour discovery against a Windows host, touch control, reconnection
and Windows end-to-end behavior remain untested.

## Native build and runtime results

| Check | Actual result | Scope |
| --- | --- | --- |
| `flutter build ios --simulator --debug --no-pub --target lib/main.dart` | **Passed**; production `Runner.app` built for arm64 iOS Simulator | Swift, Pods, Rust arm64 simulator archive, Flutter compile and link |
| `flutter build ios --simulator --debug --no-pub --target integration_test/ios_startup_test.dart` | **Passed** | Same native pipeline with on-device test entry point |
| `simctl install` and `simctl launch` of that test app on iPhone 18 Pro, iOS 27 | **Passed**; foreground first screen shown | Actual simulator install and startup |
| Simulator integration test log | **All tests passed** (`00:07 +2`) | Statically linked Rust hello and encrypted packet round trip, wrong-key rejection, iOS Keychain write/read/delete, native Bonjour channel response, onboarding, QR scanner screen and back navigation |
| Production simulator screenshot after clean relaunch | **“Find your computer”**, “Scan pairing code”, “On this Wi-Fi” and “Nothing found” shown | Rust initialization and first unpaired screen reached; no Windows host was available for discovery |
| `flutter build ios --release --no-codesign --no-pub` | **Passed**; 20.0 MB `Runner.app` built for iPhone | Unsigned arm64 device Swift, Rust, Flutter and Pods compilation/link; not installable until signing |
| Real iPhone signed install, startup, background/reopen and live camera preview | **Passed on 12 October** | Windows pairing, Bonjour discovery against a host, input control and reconnect remain untested |

The simulator launch initially failed because **Xcode 27/iOS 27 requires the
UIScene lifecycle** for apps built with this SDK. `Info.plist` now declares a
single Flutter scene, and a scene delegate hosts the pinned Flutter engine. The
next simulator build compiled, installed and launched; the production app and
integration test passed. This is a native result, not inferred from host unit tests.

Xcode 27 rejected older iOS deployment targets, so Runner and Pods use **iOS
15.0**. Flutter 3.35.4's simulator validation passed multiple architecture
names to `lipo -verify_arch`, which Xcode 27 rejects. The simulator build is
limited to the current Mac architecture; the Rust script retains support for
both simulator triples. Host Cargo build scripts receive the macOS SDK rather
than Xcode's inherited iOS SDK. `Cargo.lock` pins **libc 0.2.189** instead of
0.2.190 because the latter gates dyld functions needed by the current
`backtrace` dependency on iOS. No wire or desktop behavior was changed.

Direct `flutter test -d <simulator-id>` did not reach execution: Xcode 27
reported the named simulator destination incompatible with this Runner target,
although generic simulator compilation succeeded. The test entry point was
compiled with `flutter build ios --simulator --target ...`, installed using
`simctl` and launched. The simulator's Runner log contains the test names and
“All tests passed!”. This is an equivalent native execution of the test body;
the CLI destination issue remains a tooling limitation for the usual command.

## Host regression checks

| Check | Result | Limit |
| --- | --- | --- |
| `npm test` | 6 passed; rerun on 12 October after final diff | v1 protocol oracle |
| `cargo test --locked -p trackpad-core -p trackpad-bridge` | 23 passed; rerun on 12 October | Host Rust behavior, using final libc pin |
| `cargo clippy --locked -p trackpad-core -p trackpad-bridge --all-targets -- -D warnings` | Passed; rerun on 12 October | Host Rust lint |
| `cargo build --locked -p trackpad-bridge` | Passed | macOS library used for FFI tests |
| `flutter analyze --no-pub` | No issues; rerun on 12 October | Dart static checks, including integration test |
| Host `flutter test --no-pub` with `FRB_DART_LOAD_EXTERNAL_LIBRARY_NATIVE_LIB_DIR` | 27 passed; rerun on 12 October | Host widgets, parser and Rust FFI; separate from simulator test |
| `python3 tools/test-ios-build.py` | 2 tests passed; rerun on 12 October, covering 4 architecture/configuration subcases | Rust build script orchestration with fake tools |
| `pod install` | Passed, native dependencies resolved | Dependency integration |

`ios/Podfile`, workspace/project settings, Xcode Rust phase, scene lifecycle,
Bonjour method channel and app Keychain entitlement are included. The existing
Windows wire format and generated FRB bindings are unchanged. Current protocol
and documentation discrepancies are noted in [the iPhone guide](iphone-sideload.md).
The final signing cleanup also passed a simulator Debug build, signed device
Release build, `codesign --verify --deep --strict`, FRB export check, and physical
install/process launch. `git diff --check`, Podfile syntax and iOS plist lint passed.

## Remaining validation

Follow [the iPhone guide](iphone-sideload.md) for the Windows end-to-end
checklist. Keep simulator, physical-device and Windows results separate in any
subsequent report. Physical Debug launch under LLDB also needs further testing
or an upstream toolchain fix; the signed Release app is installed and working.
