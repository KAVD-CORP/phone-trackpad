import 'dart:io';
import 'dart:typed_data';

import 'src/rust/api.dart/api.dart' as bridge;

/// Encrypted UDP sender. Packets are sealed in Rust (same `trackpad-core`
/// as the desktop); the nonce derives from the per-session sequence, so it
/// strictly increases here. Plain UDP is gone in Phase 4.
class SecureSender {
  RawDatagramSocket? _socket;
  InternetAddress? _target;
  int _port = 51515;
  Uint8List _phoneKey = Uint8List(0);
  Uint8List _desktopKey = Uint8List(0);
  BigInt _session = BigInt.zero;
  BigInt _seq = BigInt.zero;

  bool get isReady => _socket != null && _target != null && _phoneKey.isNotEmpty;

  Future<void> connect({
    required String host,
    required int port,
    required Uint8List phoneKey,
    required Uint8List desktopKey,
    required BigInt session,
  }) async {
    _socket ??= await RawDatagramSocket.bind(InternetAddress.anyIPv4, 0);
    _target = (await InternetAddress.lookup(host))
        .firstWhere((a) => a.type == InternetAddressType.IPv4);
    _port = port;
    _phoneKey = phoneKey;
    _desktopKey = desktopKey;
    _session = session;
    _seq = BigInt.zero;
    _socket!.listen((event) {
      if (event != RawSocketEvent.read) return;
      final dg = _socket!.receive();
      if (dg == null) return;
      final out = bridge.bridgeOpen(key: _desktopKey.toList(), packet: dg.data.toList());
      if (!out.ok) return; // unauthenticated garbage: drop silently
    });
  }

  void close() {
    _socket?.close();
    _socket = null;
    _target = null;
    _phoneKey = Uint8List(0);
  }

  void _send(Uint8List bytes) {
    final s = _socket;
    final t = _target;
    if (s == null || t == null || _phoneKey.isEmpty) return;
    _seq += BigInt.one;
    s.send(bytes, t, _port);
  }

  void sendMove(double dx, double dy) => _send(bridge.bridgeSealMove(
        key: _phoneKey.toList(),
        session: _session,
        seq: _seq,
        dx: dx.round().clamp(-32768, 32767),
        dy: dy.round().clamp(-32768, 32767),
      ));

  void sendClick(int button) => _send(bridge.bridgeSealClick(
        key: _phoneKey.toList(),
        session: _session,
        seq: _seq,
        button: button,
      ));

  void sendButton(int button, bool down) {
    final bytes = bridge.bridgeSealButton(
      key: _phoneKey.toList(),
      session: _session,
      seq: _seq,
      button: button,
      down: down,
    );
    _send(bytes);
    if (!down) {
      // Redundant release against a dropped BUTTON_UP.
      _send(bridge.bridgeSealButton(
        key: _phoneKey.toList(),
        session: _session,
        seq: _seq,
        button: button,
        down: false,
      ));
    }
  }

  void sendScroll(int dx, int dy) => _send(bridge.bridgeSealScroll(
        key: _phoneKey.toList(),
        session: _session,
        seq: _seq,
        dx: dx.clamp(-32768, 32767),
        dy: dy.clamp(-32768, 32767),
      ));
}
