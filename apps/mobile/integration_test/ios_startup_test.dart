import 'dart:io';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:trackpad_mobile/discovery.dart';
import 'package:trackpad_mobile/main.dart' as app;
import 'package:trackpad_mobile/src/rust/api.dart/api.dart' as bridge;

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('iOS native startup, Rust crypto, Keychain and unpaired screens', (
    tester,
  ) async {
    expect(
      Platform.isIOS,
      isTrue,
      reason: 'Run on an iOS simulator or iPhone.',
    );
    await app.main();
    await tester.pumpAndSettle(
      const Duration(milliseconds: 100),
      EnginePhase.sendSemanticsUpdate,
      const Duration(seconds: 30),
    );

    // Exercises the native Bonjour channel even when no Windows host is present.
    final nearby = await browseIosComputers(
      timeout: const Duration(milliseconds: 250),
    );
    expect(nearby, isA<List<FoundComputer>>());

    // Calls the actual static Rust library loaded by production startup.
    expect(await bridge.bridgeHello(), contains('trackpad-core hello'));
    final key = List<int>.filled(32, 7);
    final packet = bridge.bridgeSealMove(
      key: key,
      session: BigInt.from(5),
      seq: BigInt.from(9),
      dx: 100,
      dy: -50,
    );
    final opened = bridge.bridgeOpen(key: key, packet: packet.toList());
    expect(opened.ok, isTrue, reason: opened.error);
    expect(opened.a, 100);
    expect(opened.b, -50);
    expect(
      bridge.bridgeOpen(key: List.filled(32, 8), packet: packet.toList()).ok,
      isFalse,
    );

    const storage = FlutterSecureStorage();
    final probe = 'ios_smoke_${DateTime.now().microsecondsSinceEpoch}';
    try {
      await storage.write(key: probe, value: 'native-keychain-round-trip');
      expect(await storage.read(key: probe), 'native-keychain-round-trip');
    } finally {
      await storage.delete(key: probe);
    }

    // Does not fabricate a trusted computer or send input to any host.
    if (find.text('Pair a different computer').evaluate().isNotEmpty) {
      await tester.tap(find.text('Pair a different computer'));
      await tester.pumpAndSettle();
    }
    expect(find.text('Find your computer'), findsOneWidget);
    expect(find.text('Scan pairing code'), findsOneWidget);
    await tester.tap(find.text('Scan pairing code'));
    await tester.pumpAndSettle();
    expect(find.byType(MobileScanner), findsOneWidget);
    expect(
      find.text('Point the camera at the code on your computer.'),
      findsOneWidget,
    );
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.text('Find your computer'), findsOneWidget);
  });
}
