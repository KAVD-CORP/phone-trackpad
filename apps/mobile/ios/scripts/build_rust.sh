#!/bin/bash
# Invoked by Xcode before Runner links. Cargo performs incremental rebuilds.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/../../../.." && pwd)"
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
command -v cargo >/dev/null || { echo "error: Install Rust 1.90.0; see docs/iphone-sideload.md" >&2; exit 1; }
cd "$repo_root"
case "${CONFIGURATION:?}" in
  Debug*) profile=debug; profile_args=(--profile dev) ;;
  Release*|Profile*) profile=release; profile_args=(--release) ;;
  *) echo "error: Unsupported configuration $CONFIGURATION" >&2; exit 1 ;;
esac
: "${PLATFORM_NAME:?}" "${ARCHS:?}" "${BUILT_PRODUCTS_DIR:?}"
export CARGO_TARGET_DIR="${TARGET_TEMP_DIR:?}/trackpad-rust"
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}"
# Cargo also compiles macOS build scripts and procedural macros. Inheriting
# Xcode's iOS SDKROOT makes their host linker look in the wrong SDK.
# Host clang needs an explicit macOS sysroot when invoked directly from Xcode.
# Rust/cc reject that SDK for iOS targets and resolve their SDK via xcrun.
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
libraries=()
for arch in $ARCHS; do
  case "$PLATFORM_NAME/$arch" in
    iphoneos/arm64) target=aarch64-apple-ios ;;
    iphonesimulator/arm64) target=aarch64-apple-ios-sim ;;
    iphonesimulator/x86_64) target=x86_64-apple-ios ;;
    *) echo "error: Unsupported iOS architecture $PLATFORM_NAME/$arch" >&2; exit 1 ;;
  esac
  # Do not download toolchains in Xcode; setup installs the targets explicitly.
  cargo build --locked -p trackpad-bridge --lib --target "$target" "${profile_args[@]}"
  libraries+=("$CARGO_TARGET_DIR/$target/$profile/libtrackpad_bridge.a")
done
mkdir -p "$BUILT_PRODUCTS_DIR"
xcrun lipo -create "${libraries[@]}" -output "$BUILT_PRODUCTS_DIR/libtrackpad_bridge.a"
