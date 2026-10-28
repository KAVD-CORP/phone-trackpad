import 'dart:typed_data';

/// TCP framing for Handsfree. The hello is the existing pairing `u16` frame.
/// The body is a `u32` frame because a voice clip does not fit in 8 KiB.
const handsfreeHello = 'HANDSFREE';
const handsfreeFrameMax = 8192;
const handsfreeBodyMax = 1048576 + 64;

Uint8List encodeU16Frame(List<int> payload) {
  if (payload.length > handsfreeFrameMax) {
    throw ArgumentError('frame too large');
  }
  final out = BytesBuilder(copy: false);
  final len = ByteData(2)..setUint16(0, payload.length, Endian.big);
  out.add(len.buffer.asUint8List());
  out.add(payload);
  return out.toBytes();
}

Uint8List encodeU32Frame(List<int> payload) {
  if (payload.length > handsfreeBodyMax) {
    throw ArgumentError('frame too large');
  }
  final out = BytesBuilder(copy: false);
  final len = ByteData(4)..setUint32(0, payload.length, Endian.big);
  out.add(len.buffer.asUint8List());
  out.add(payload);
  return out.toBytes();
}

/// Reads one big-endian length prefix and that many bytes from [chunks].
/// Returns null until the frame is complete.
class FrameBuffer {
  final int _width;
  final List<int> _buf = [];

  FrameBuffer.u32() : _width = 4;

  int? _need;

  Uint8List? add(List<int> chunk) {
    _buf.addAll(chunk);
    _need ??= _takeLength();
    final need = _need;
    if (need == null || _buf.length < need) return null;
    final body = Uint8List.fromList(_buf.sublist(0, need));
    _buf.removeRange(0, need);
    _need = null;
    return body;
  }

  int? _takeLength() {
    if (_buf.length < _width) return null;
    final data = ByteData.sublistView(
      Uint8List.fromList(_buf.sublist(0, _width)),
    );
    final n = data.getUint32(0, Endian.big);
    _buf.removeRange(0, _width);
    if (n > handsfreeBodyMax) {
      throw StateError('Handsfree reply is too large');
    }
    return n;
  }
}
