# Mobile app

Shared Flutter app for iOS and Android: discovery, QR/short-code pairing,
trackpad and settings. Flutter **3.35.4**, Dart **3.9.2**, FRB **2.13.0**,
Rust **1.90.0**. The platform folders are committed; do not regenerate them.

See [iPhone setup, signing and testing](../../docs/iphone-sideload.md) for the
complete Mac/iPhone procedure and validation limits.

From the repository root:

```bash
cargo build --locked -p trackpad-bridge
cd apps/mobile
flutter pub get
flutter analyze
# macOS/Linux host FFI tests (use the equivalent environment variable on Windows):
FRB_DART_LOAD_EXTERNAL_LIBRARY_NATIVE_LIB_DIR="$PWD/../../target/debug" flutter test
flutter build ios --simulator --debug
flutter run -d <iphone-or-simulator-id>
```

The iOS Xcode build compiles and links Rust automatically. The host `cargo build`
above is for Flutter's FFI tests, not for the iOS app.

Regenerate bindings only after bridge API changes, from the repository root:

```bash
cargo install flutter_rust_bridge_codegen --version 2.13.0 --locked
flutter_rust_bridge_codegen generate
```

Generated bindings live in `lib/src/rust/api.dart/` (the existing directory name)
and `crates/trackpad-bridge/src/frb_generated.rs`. Keep both sides on 2.13.0.
