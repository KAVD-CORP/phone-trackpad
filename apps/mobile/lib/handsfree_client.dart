import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'handsfree_frame.dart';
import 'src/rust/api.dart/api.dart' as bridge;

const handsfreeVoice = 1;
const handsfreeCommand = 2;
const handsfreeTypeText = 3;
const handsfreeKeyCombo = 4;
const handsfreeCancel = 5;

/// Sends one sealed Handsfree command to the paired desktop.
class HandsfreeClient {
  final String host;
  final int port;
  final Uint8List phoneKey;
  final BigInt sessionId;
  BigInt _seq = BigInt.one;

  HandsfreeClient({
    required this.host,
    required this.port,
    required this.phoneKey,
    required this.sessionId,
  });

  Future<String> command(String text) =>
      _send(handsfreeCommand, utf8.encode(text));

  Future<String> typeText(String text) =>
      _send(handsfreeTypeText, utf8.encode(text));

  Future<String> keyCombo(String combo) =>
      _send(handsfreeKeyCombo, utf8.encode(combo));

  Future<String> cancel() => _send(handsfreeCancel, Uint8List(0));

  Future<String> voiceWav(Uint8List wav) {
    if (wav.length < 12 ||
        utf8.decode(wav.sublist(0, 4), allowMalformed: true) != 'RIFF') {
      throw StateError('Voice must be a WAV recording.');
    }
    return _send(handsfreeVoice, wav);
  }

  Future<String> _send(int kind, List<int> payload) async {
    if (sessionId == BigInt.zero) {
      throw StateError('Handsfree needs a paired session.');
    }
    final seq = _seq;
    _seq += BigInt.one;
    final sealed = bridge.bridgeSealHandsfree(
      key: phoneKey,
      session: sessionId,
      seq: seq,
      kind: kind,
      payload: payload,
    );
    if (sealed.isEmpty) {
      throw StateError(
        'Could not seal the Handsfree command. Nothing was sent.',
      );
    }
    final socket = await Socket.connect(
      host,
      port,
      timeout: const Duration(seconds: 8),
    );
    try {
      socket.add(encodeU16Frame(utf8.encode(handsfreeHello)));
      socket.add(encodeU32Frame(sealed));
      await socket.flush();
      return await _readReply(socket).timeout(const Duration(seconds: 90));
    } finally {
      await socket.close();
    }
  }

  Future<String> _readReply(Socket socket) async {
    final frames = FrameBuffer.u32();
    await for (final chunk in socket) {
      final body = frames.add(chunk);
      if (body != null) return utf8.decode(body);
    }
    throw StateError('The computer closed the Handsfree connection.');
  }
}
