import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:trackpad_mobile/handsfree_frame.dart';

void main() {
  test('hello frame is a big-endian u16 pairing frame', () {
    final bytes = encodeU16Frame(handsfreeHello.codeUnits);
    expect(bytes.sublist(0, 2), [0, handsfreeHello.length]);
    expect(String.fromCharCodes(bytes.sublist(2)), handsfreeHello);
  });

  test('body frame length is a big-endian u32', () {
    final bytes = encodeU32Frame([0x68, 0x69]);
    final len = ByteData.sublistView(bytes).getUint32(0);
    expect(len, 2);
    expect(bytes.sublist(4), [0x68, 0x69]);
  });

  test('reply buffer waits for the declared length', () {
    final buf = FrameBuffer.u32();
    expect(buf.add([0, 0, 0]), isNull);
    final body = buf.add([2, 0x4f, 0x4b]);
    expect(body, isNotNull);
    expect(String.fromCharCodes(body!), 'OK');
  });
}
