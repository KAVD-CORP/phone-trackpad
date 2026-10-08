import 'package:flutter_test/flutter_test.dart';
import 'package:trackpad_mobile/src/rust/api.dart/api.dart' as bridge;
import 'package:trackpad_mobile/src/rust/api.dart/frb_generated.dart';

/// FFI smoke test: loads the real compiled Rust core through the generated
/// FRB bindings and round-trips crypto. This is the exact code path the
/// phone uses (same crate, same bindings); only the .so packaging for
/// iOS/Android differs (Phase 5 store builds).
///
/// Requires the cdylib: `cargo build -p trackpad-bridge`, with
/// `target/debug` on PATH so the loader finds `trackpad_bridge.dll`.
void main() {
  test('FRB seal/open round trip through the real Rust core', () async {
    await RustLib.init();
    final kp = bridge.bridgeGenKeypair();
    expect(kp.privateKey.length, 32);
    expect(kp.publicKey.length, 32);

    final key = List<int>.filled(32, 7);
    final pkt = bridge.bridgeSealMove(
      key: key,
      session: BigInt.from(5),
      seq: BigInt.from(9),
      dx: 100,
      dy: -50,
    );
    expect(pkt, isNotEmpty);

    final out = bridge.bridgeOpen(key: key, packet: pkt.toList());
    expect(out.ok, isTrue, reason: out.error);
    expect(out.session, BigInt.from(5));
    expect(out.seq, BigInt.from(9));
    expect(out.msgType, 0x01);
    expect(out.a, 100);
    expect(out.b, -50);

    final wrong = bridge.bridgeOpen(
      key: List<int>.filled(32, 8),
      packet: pkt.toList(),
    );
    expect(wrong.ok, isFalse);
  });

  test('FRB QR decode validates structure', () {
    final bad = bridge.bridgeQrDecode(qrText: 'not-a-qr', nowUnix: BigInt.zero);
    expect(bad.ok, isFalse);
  });
}
