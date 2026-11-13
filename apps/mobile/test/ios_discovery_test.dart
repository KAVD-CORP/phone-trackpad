import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:trackpad_mobile/discovery.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('com.kavd.phonetrackpad/bonjour');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test('iOS discovery preserves ports and the pairing fingerprint', () async {
    messenger.setMockMethodCallHandler(channel, (call) async {
      expect(call.method, 'browse');
      expect(call.arguments, {'timeoutMs': 750});
      return [
        {
          'name': 'Windows PC',
          'host': '192.168.1.20',
          'tcpPort': 51516,
          'udpPort': 51515,
          'fingerprint': base64Url.encode(List.filled(32, 9)),
        },
        {
          'name': 'bad address',
          'host': 'invalid',
          'tcpPort': 51516,
          'udpPort': 51515,
        },
        {
          'name': 'bad port',
          'host': '192.168.1.21',
          'tcpPort': 0,
          'udpPort': 51515,
        },
      ];
    });
    final found = await browseIosComputers(
      timeout: const Duration(milliseconds: 750),
    );
    expect(found, hasLength(1));
    expect(found.single.name, 'Windows PC');
    expect(found.single.host, '192.168.1.20');
    expect(found.single.tcpPort, 51516);
    expect(found.single.udpPort, 51515);
    expect(found.single.fingerprint, List.filled(32, 9));
  });

  test('malformed fingerprints cannot authorize short-code pairing', () async {
    messenger.setMockMethodCallHandler(
      channel,
      (_) async => [
        {
          'name': 'PC',
          'host': '192.168.1.20',
          'tcpPort': 51516,
          'udpPort': 51515,
          'fingerprint': 'not a fingerprint',
        },
      ],
    );
    expect((await browseIosComputers()).single.fingerprint, isNull);
  });

  test('native discovery failure leaves QR fallback available', () async {
    messenger.setMockMethodCallHandler(
      channel,
      (_) async => throw PlatformException(code: 'discovery_unavailable'),
    );
    expect(await browseIosComputers(), isEmpty);
  });
}
