"""Exercise Xcode-to-Cargo target selection without pretending to compile iOS."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'apps/mobile/ios/scripts/build_rust.sh'


class IOSBuildScriptTests(unittest.TestCase):
    def build(self, platform, arches, configuration):
        with tempfile.TemporaryDirectory(prefix='trackpad build ') as directory:
            root = Path(directory)
            binaries = root / 'bin'
            binaries.mkdir()
            (binaries / 'cargo').write_text('''#!/bin/bash
set -eu
[[ "$SDKROOT" == "/fake/macOS SDK" ]] # Host compiler uses the macOS SDK.
printf '%s\\n' "$*" >> "$TEST_LOG"
[[ "$*" == *"--locked -p trackpad-bridge --lib"* ]]
while [[ $# -gt 0 ]]; do
  if [[ "$1" == --target ]]; then target="$2"; fi
  shift
done
profile=debug
[[ "$CONFIGURATION" == Debug* ]] || profile=release
mkdir -p "$CARGO_TARGET_DIR/$target/$profile"
touch "$CARGO_TARGET_DIR/$target/$profile/libtrackpad_bridge.a"
''')
            (binaries / 'xcrun').write_text('''#!/bin/bash
set -eu
if [[ "$1" == --sdk ]]; then [[ "$2" == macosx ]]; printf '/fake/macOS SDK\\n'; exit; fi
[[ "$1" == lipo && "$2" == -create ]]
shift 2
while [[ "$1" != -output ]]; do [[ -f "$1" ]]; shift; done
touch "$2"
''')
            for executable in binaries.iterdir():
                executable.chmod(0o755)
            env = dict(os.environ, PATH=f'{binaries}:/usr/bin:/bin',
                       CARGO_HOME=str(root), SDKROOT='/fake/iPhoneSimulator.sdk',
                       PLATFORM_NAME=platform, ARCHS=arches,
                       CONFIGURATION=configuration, TARGET_TEMP_DIR=str(root / 'temporary files'),
                       BUILT_PRODUCTS_DIR=str(root / 'build products'), TEST_LOG=str(root / 'calls'))
            result = subprocess.run(['/bin/bash', str(SCRIPT)], env=env, capture_output=True, text=True)
            calls = (root / 'calls').read_text() if (root / 'calls').exists() else ''
            archive = (root / 'build products/libtrackpad_bridge.a').exists()
            return result, calls, archive

    def test_targets_profiles_and_paths_with_spaces(self):
        cases = [
            ('iphoneos', 'arm64', 'Release', ['aarch64-apple-ios'], True),
            ('iphonesimulator', 'arm64', 'Debug', ['aarch64-apple-ios-sim'], False),
            ('iphonesimulator', 'x86_64', 'Profile', ['x86_64-apple-ios'], True),
            ('iphonesimulator', 'arm64 x86_64', 'Debug', ['aarch64-apple-ios-sim', 'x86_64-apple-ios'], False),
        ]
        for platform, arches, configuration, targets, release in cases:
            with self.subTest(platform=platform, arches=arches, configuration=configuration):
                result, calls, archive = self.build(platform, arches, configuration)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(archive)
                self.assertEqual(len(calls.splitlines()), len(targets))
                for target in targets:
                    self.assertIn(f'--target {target}', calls)
                self.assertEqual('--release' in calls, release)

    def test_unsupported_platform_fails_before_link(self):
        result, _, archive = self.build('macosx', 'arm64', 'Debug')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Unsupported iOS architecture', result.stderr)
        self.assertFalse(archive)


if __name__ == '__main__':
    unittest.main()
