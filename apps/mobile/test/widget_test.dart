import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:trackpad_mobile/discovery.dart';
import 'package:trackpad_mobile/main.dart';
import 'package:trackpad_mobile/session.dart';
import 'package:trackpad_mobile/trackpad_screen.dart';

class _FakeSessions extends SessionManager {
  @override
  bool get isPaired => false;
}

FoundComputer _fakePc() => FoundComputer(
      name: 'test-pc',
      address: InternetAddress('192.168.1.2'),
      tcpPort: 51516,
      udpPort: 51515,
      fingerprint: Uint8List(32),
    );

void main() {
  testWidgets('onboarding shows scan button and discovered computers', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: OnboardingScreen(sessions: _FakeSessions(), browse: () async => [_fakePc()]),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Scan pairing code'), findsOneWidget);
    expect(find.text('test-pc'), findsOneWidget);
  });

  testWidgets('onboarding empty state without network', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: OnboardingScreen(sessions: _FakeSessions(), browse: () async => []),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.textContaining('Nothing found'), findsOneWidget);
  });

  testWidgets('code screen renders address and code fields', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: CodeScreen(
          sessions: _FakeSessions(),
          computer: _fakePc(),
          onPaired: (_) async {},
        ),
      ),
    );
    expect(find.text('Computer address'), findsOneWidget);
    expect(find.text('6-digit code'), findsOneWidget);
  });

  testWidgets('trackpad screen renders surface and settings open', (tester) async {
    final sessions = _FakeSessions();
    final session = LiveSession(
      computer: TrustedComputer(
        name: 'pc',
        host: '127.0.0.1',
        tcpPort: 51516,
        udpPort: 51515,
        serverFp: Uint8List(32),
      ),
      phoneKey: Uint8List(32),
      desktopKey: Uint8List(32),
      sessionId: BigInt.one,
    );
    await tester.pumpWidget(
      MaterialApp(home: TrackpadScreen(sessions: sessions, session: session)),
    );
    await tester.pump();
    expect(find.textContaining('Drag to move'), findsOneWidget);
    await tester.tap(find.byTooltip('Pointer settings'));
    await tester.pumpAndSettle();
    expect(find.textContaining('Pointer sensitivity'), findsOneWidget);
  });
}
