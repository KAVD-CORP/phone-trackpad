import 'dart:io';

import 'package:record/record.dart';

/// Push-to-talk WAV capture. Constructed only when the user taps Speak,
/// so widget tests that never tap it do not touch the microphone plugin.
class VoiceCapture {
  AudioRecorder? _recorder;

  Future<void> start(String path) async {
    final recorder = _recorder ??= AudioRecorder();
    final allowed = await recorder.hasPermission();
    if (!allowed) {
      throw StateError('Microphone permission was denied.');
    }
    await recorder.start(
      const RecordConfig(
        encoder: AudioEncoder.wav,
        sampleRate: 16000,
        numChannels: 1,
      ),
      path: path,
    );
  }

  Future<String?> stop() async {
    final recorder = _recorder;
    if (recorder == null) return null;
    return recorder.stop();
  }

  Future<void> dispose() async {
    await _recorder?.dispose();
    _recorder = null;
  }
}

String voiceClipPath() {
  final stamp = DateTime.now().millisecondsSinceEpoch;
  return '${Directory.systemTemp.path}/handsfree-$stamp.wav';
}
