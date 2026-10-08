import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'src/rust/api.dart/api.dart' as bridge;

/// One trusted computer (MVP: exactly one; multi-computer is Phase 7).
class TrustedComputer {
  final String name;
  final String host;
  final int tcpPort;
  final int udpPort;
  final Uint8List serverFp;

  const TrustedComputer({
    required this.name,
    required this.host,
    required this.tcpPort,
    required this.udpPort,
    required this.serverFp,
  });
}

/// Live input session after pairing or reconnect.
class LiveSession {
  final TrustedComputer computer;
  final Uint8List phoneKey;
  final Uint8List desktopKey;
  final BigInt sessionId;

  const LiveSession({
    required this.computer,
    required this.phoneKey,
    required this.desktopKey,
    required this.sessionId,
  });
}

/// Owns keys, trust, and the pairing ceremonies. All crypto runs in Rust
/// through the FRB bindings (same `trackpad-core` as the desktop).
class SessionManager extends ChangeNotifier {
  static const _kPriv = 'client_priv_hex';
  static const _kPub = 'client_pub_hex';
  static const _kName = 'computer_name';
  static const _kHost = 'computer_host';
  static const _kTcp = 'computer_tcp';
  static const _kUdp = 'computer_udp';
  static const _kFp = 'computer_fp_hex';

  final FlutterSecureStorage _storage = const FlutterSecureStorage();

  Uint8List? _clientPriv;
  TrustedComputer? _computer;
  LiveSession? _session;

  TrustedComputer? get computer => _computer;
  LiveSession? get session => _session;
  bool get isPaired => _computer != null;
  bool get isConnected => _session != null;

  static String deviceName() =>
      Platform.isIOS ? 'iPhone' : (Platform.isAndroid ? 'Android phone' : 'phone');

  String _hex(Uint8List b) =>
      b.map((x) => x.toRadixString(16).padLeft(2, '0')).join();

  Uint8List _unhex(String s) {
    final out = Uint8List(s.length ~/ 2);
    for (var i = 0; i < out.length; i++) {
      out[i] = int.parse(s.substring(i * 2, i * 2 + 2), radix: 16);
    }
    return out;
  }

  /// Load or create the long-term client keypair. Never leaves secure storage.
  Future<void> init() async {
    final privHex = await _storage.read(key: _kPriv);
    final pubHex = await _storage.read(key: _kPub);
    if (privHex != null && pubHex != null) {
      _clientPriv = _unhex(privHex);
    } else {
      final kp = bridge.bridgeGenKeypair();
      if (kp.privateKey.isEmpty) throw StateError('keygen failed');
      _clientPriv = kp.privateKey;
      await _storage.write(key: _kPriv, value: _hex(kp.privateKey));
      await _storage.write(key: _kPub, value: _hex(kp.publicKey));
    }
    final name = await _storage.read(key: _kName);
    final host = await _storage.read(key: _kHost);
    final tcp = await _storage.read(key: _kTcp);
    final udp = await _storage.read(key: _kUdp);
    final fp = await _storage.read(key: _kFp);
    if (name != null && host != null && tcp != null && udp != null && fp != null) {
      _computer = TrustedComputer(
        name: name,
        host: host,
        tcpPort: int.parse(tcp),
        udpPort: int.parse(udp),
        serverFp: _unhex(fp),
      );
    }
    notifyListeners();
  }

  Future<LiveSession> _store(
    bridge.PairOut out,
    String name,
    String host,
    int tcpPort,
    int udpPort,
    Uint8List serverFp,
  ) async {
    if (!out.ok) throw StateError(out.error);
    await _storage.write(key: _kName, value: name);
    await _storage.write(key: _kHost, value: host);
    await _storage.write(key: _kTcp, value: '$tcpPort');
    await _storage.write(key: _kUdp, value: '$udpPort');
    await _storage.write(key: _kFp, value: _hex(serverFp));
    _computer = TrustedComputer(
      name: name,
      host: host,
      tcpPort: tcpPort,
      udpPort: udpPort,
      serverFp: serverFp,
    );
    _session = LiveSession(
      computer: _computer!,
      phoneKey: out.phoneKey,
      desktopKey: out.desktopKey,
      sessionId: out.sessionHint != BigInt.zero
          ? out.sessionHint
          : BigInt.from(DateTime.now().millisecondsSinceEpoch),
    );
    notifyListeners();
    return _session!;
  }

  /// QR ceremony. Blocks until the desktop approves/denies/times out.
  Future<LiveSession> pairWithQr(String qrText, String tcpHost) async {
    final decoded = bridge.bridgeQrDecode(
      qrText: qrText,
      nowUnix: BigInt.from(DateTime.now().millisecondsSinceEpoch ~/ 1000),
    );
    if (!decoded.ok) throw StateError(decoded.error);
    final out = bridge.bridgePairQr(
      qrText: qrText,
      tcpHost: tcpHost,
      deviceName: deviceName(),
      clientPriv: _clientPriv!.toList(),
    );
    return _store(
      out,
      decoded.deviceName,
      decoded.host,
      decoded.tcpPort,
      decoded.udpPort,
      decoded.serverFp,
    );
  }

  /// Short-code ceremony against a discovered (or manual) host.
  /// `serverFp` binds the code to the desktop (QR flow carries it; the
  /// code screen gets it from discovery or manual entry).
  Future<LiveSession> pairWithCode({
    required String host,
    required int tcpPort,
    required int udpPort,
    required String code,
    required Uint8List serverFp,
    required String serverName,
  }) async {
    final out = bridge.bridgePairCode(
      host: host,
      tcpPort: tcpPort,
      code: code,
      serverFp: serverFp.toList(),
      serverName: serverName,
      deviceName: deviceName(),
      clientPriv: _clientPriv!.toList(),
    );
    return _store(out, serverName, host, tcpPort, udpPort, serverFp);
  }

  /// Reconnect to the stored computer. No approval, no ceremony.
  Future<LiveSession> reconnect() async {
    final c = _computer;
    if (c == null) throw StateError('no trusted computer');
    final out = bridge.bridgeReconnect(
      host: c.host,
      tcpPort: c.tcpPort,
      clientPriv: _clientPriv!.toList(),
      serverFp: c.serverFp.toList(),
      serverName: c.name,
      deviceName: deviceName(),
    );
    if (!out.ok) throw StateError(out.error);
    _session = LiveSession(
      computer: c,
      phoneKey: out.phoneKey,
      desktopKey: out.desktopKey,
      sessionId: out.sessionHint != BigInt.zero
          ? out.sessionHint
          : BigInt.from(DateTime.now().millisecondsSinceEpoch),
    );
    notifyListeners();
    return _session!;
  }

  void disconnected() {
    _session = null;
    notifyListeners();
  }

  /// Forget the computer locally (desktop-side revoke is separate, in the
  /// desktop UI — revoking there kills the session even if we remember it).
  Future<void> forget() async {
    for (final k in [_kName, _kHost, _kTcp, _kUdp, _kFp]) {
      await _storage.delete(key: k);
    }
    _computer = null;
    _session = null;
    notifyListeners();
  }
}
