import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';

import 'gesture.dart';
import 'handsfree_client.dart';
import 'handsfree_voice.dart';
import 'net.dart';
import 'session.dart';

/// Phase 4 trackpad screen: gesture engine → sealed UDP sender.
class TrackpadScreen extends StatefulWidget {
  final SessionManager sessions;
  final LiveSession session;

  const TrackpadScreen({
    super.key,
    required this.sessions,
    required this.session,
  });

  @override
  State<TrackpadScreen> createState() => _TrackpadScreenState();
}

class _TrackpadScreenState extends State<TrackpadScreen> {
  final _sender = SecureSender();
  final _settings = TrackpadSettings();
  late GestureEngine _engine = GestureEngine(_settings);
  final _command = TextEditingController();
  final _typeText = TextEditingController();
  final _voice = VoiceCapture();
  String _status = 'Connecting…';
  String _handsfree = '';
  bool _busy = false;
  bool _listening = false;
  Timer? _holdTimer;

  @override
  void initState() {
    super.initState();
    final s = widget.session;
    _sender
        .connect(
          host: s.computer.host,
          port: s.computer.udpPort,
          phoneKey: s.phoneKey,
          desktopKey: s.desktopKey,
          session: s.sessionId,
        )
        .then(
          (_) => setState(() => _status = 'Connected to ${s.computer.name}'),
        )
        .catchError((Object e) {
          setState(() => _status = 'Could not reach ${s.computer.name} ($e)');
        });
  }

  @override
  void dispose() {
    _holdTimer?.cancel();
    _sender.close();
    _command.dispose();
    _typeText.dispose();
    unawaited(_voice.dispose());
    super.dispose();
  }

  HandsfreeClient _client() {
    final s = widget.session;
    return HandsfreeClient(
      host: s.computer.host,
      port: s.computer.tcpPort,
      phoneKey: s.phoneKey,
      sessionId: s.sessionId,
    );
  }

  Future<void> _runHandsfree(Future<String> action) async {
    setState(() {
      _busy = true;
      _handsfree = 'Handsfree is working…';
    });
    try {
      final reply = await action;
      if (!mounted) return;
      setState(() => _handsfree = reply);
    } catch (e) {
      if (!mounted) return;
      setState(() => _handsfree = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _toggleVoice() async {
    if (_listening) {
      setState(() => _listening = false);
      try {
        final path = await _voice.stop();
        final file = path == null ? null : File(path);
        if (file == null || !file.existsSync() || file.lengthSync() < 44) {
          setState(() => _handsfree = 'No speech was captured.');
          return;
        }
        final wav = await file.readAsBytes();
        await _runHandsfree(_client().voiceWav(wav));
      } catch (e) {
        if (mounted) setState(() => _handsfree = '$e');
      }
      return;
    }
    try {
      final path = voiceClipPath();
      await _voice.start(path);
      if (!mounted) return;
      setState(() {
        _listening = true;
        _handsfree = 'Listening… tap again to send.';
      });
    } catch (e) {
      if (mounted) setState(() => _handsfree = '$e');
    }
  }

  void _openKeyboard() {
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (ctx) => Padding(
        padding: EdgeInsets.only(bottom: MediaQuery.viewInsetsOf(ctx).bottom),
        child: _KeyboardSheet(
          controller: _typeText,
          onType: (text) {
            Navigator.of(ctx).pop();
            _runHandsfree(_client().typeText(text));
          },
          onCombo: (combo) {
            Navigator.of(ctx).pop();
            _runHandsfree(_client().keyCombo(combo));
          },
        ),
      ),
    );
  }

  void _emit(List<GestureOut> outs) {
    if (!_sender.isReady) return;
    for (final o in outs) {
      switch (o) {
        case OutMove(dx: final dx, dy: final dy):
          _sender.sendMove(dx.toDouble(), dy.toDouble());
        case OutScroll(dx: final dx, dy: final dy):
          _sender.sendScroll(dx, dy);
        case OutClick(button: final b):
          _sender.sendClick(b);
        case OutDown(button: final b):
          _sender.sendButton(b, true);
        case OutUp(button: final b):
          _sender.sendButton(b, false);
      }
    }
  }

  int _now() => DateTime.now().millisecondsSinceEpoch;

  void _ensureHoldTimer() {
    _holdTimer ??= Timer.periodic(
      const Duration(milliseconds: 50),
      (_) => _emit(_engine.tick(_now())),
    );
  }

  void _stopHoldTimer() {
    _holdTimer?.cancel();
    _holdTimer = null;
  }

  Future<void> _disconnect() async {
    widget.sessions.disconnected();
    if (mounted) Navigator.of(context).pop();
  }

  void _openSettings() {
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (ctx) => _SettingsSheet(
        settings: _settings,
        onChanged: () {
          setState(() => _engine = GestureEngine(_settings));
          Navigator.of(ctx).pop();
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.session.computer.name),
        actions: [
          IconButton(
            icon: const Icon(Icons.tune),
            tooltip: 'Pointer settings',
            onPressed: _openSettings,
          ),
          IconButton(
            icon: const Icon(Icons.link_off),
            tooltip: 'Disconnect',
            onPressed: _disconnect,
          ),
        ],
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
            child: Text(_status, semanticsLabel: 'Connection status'),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(8, 0, 8, 4),
            child: Column(
              children: [
                TextField(
                  controller: _command,
                  decoration: const InputDecoration(
                    isDense: true,
                    hintText: 'Say a command, or type it',
                  ),
                ),
                Wrap(
                  children: [
                    IconButton(
                      tooltip: _listening
                          ? 'Send voice command'
                          : 'Speak a command',
                      onPressed: _busy ? null : _toggleVoice,
                      icon: Icon(_listening ? Icons.stop_circle : Icons.mic),
                    ),
                    IconButton(
                      tooltip: 'Run command',
                      onPressed: _busy
                          ? null
                          : () =>
                                _runHandsfree(_client().command(_command.text)),
                      icon: const Icon(Icons.send),
                    ),
                    IconButton(
                      tooltip: 'Keyboard',
                      onPressed: _openKeyboard,
                      icon: const Icon(Icons.keyboard),
                    ),
                    if (_busy)
                      IconButton(
                        tooltip: 'Cancel Handsfree',
                        onPressed: () => _runHandsfree(_client().cancel()),
                        icon: const Icon(Icons.cancel),
                      ),
                  ],
                ),
                if (_handsfree.isNotEmpty)
                  Align(
                    alignment: Alignment.centerLeft,
                    child: Text(
                      _handsfree,
                      maxLines: 3,
                      overflow: TextOverflow.ellipsis,
                      semanticsLabel: 'Handsfree status',
                    ),
                  ),
              ],
            ),
          ),
          Expanded(
            child: Listener(
              onPointerDown: (e) {
                _ensureHoldTimer();
                _emit(
                  _engine.pointerDown(
                    e.pointer,
                    e.position.dx,
                    e.position.dy,
                    _now(),
                  ),
                );
              },
              onPointerMove: (e) {
                _emit(
                  _engine.pointerMove(
                    e.pointer,
                    e.delta.dx,
                    e.delta.dy,
                    _now(),
                  ),
                );
              },
              onPointerUp: (e) {
                _emit(
                  _engine.pointerUp(
                    e.pointer,
                    e.position.dx,
                    e.position.dy,
                    _now(),
                  ),
                );
                _stopHoldTimer();
              },
              onPointerCancel: (e) {
                _emit(
                  _engine.pointerUp(
                    e.pointer,
                    e.position.dx,
                    e.position.dy,
                    _now(),
                  ),
                );
                _stopHoldTimer();
              },
              child: Container(
                width: double.infinity,
                color: Theme.of(context).colorScheme.surfaceContainerLow,
                child: const Center(
                  child: Text(
                    'Drag to move · tap to click\n'
                    'Two-finger tap: right click · two-finger drag: scroll\n'
                    'Tap, press-hold and drag to drag items.',
                    textAlign: TextAlign.center,
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _KeyboardSheet extends StatelessWidget {
  final TextEditingController controller;
  final ValueChanged<String> onType;
  final ValueChanged<String> onCombo;

  const _KeyboardSheet({
    required this.controller,
    required this.onType,
    required this.onCombo,
  });

  @override
  Widget build(BuildContext context) {
    const combos = [
      'enter',
      'tab',
      'escape',
      'backspace',
      'ctrl+a',
      'ctrl+c',
      'ctrl+v',
      'ctrl+enter',
    ];
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 8, 20, 32),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text('Type into the computer'),
          TextField(
            controller: controller,
            decoration: const InputDecoration(hintText: 'Text to type'),
            onSubmitted: onType,
          ),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            children: [
              for (final combo in combos)
                ActionChip(label: Text(combo), onPressed: () => onCombo(combo)),
            ],
          ),
        ],
      ),
    );
  }
}

class _SettingsSheet extends StatefulWidget {
  final TrackpadSettings settings;
  final VoidCallback onChanged;

  const _SettingsSheet({required this.settings, required this.onChanged});

  @override
  State<_SettingsSheet> createState() => _SettingsSheetState();
}

class _SettingsSheetState extends State<_SettingsSheet> {
  @override
  Widget build(BuildContext context) {
    final s = widget.settings;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 8, 20, 32),
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('Pointer sensitivity: ${s.sensitivity.toStringAsFixed(1)}'),
            Slider(
              value: s.sensitivity,
              min: 0.2,
              max: 3.0,
              divisions: 14,
              label: s.sensitivity.toStringAsFixed(1),
              onChanged: (v) => setState(() => s.sensitivity = v),
            ),
            SwitchListTile(
              title: const Text('Pointer acceleration'),
              subtitle: const Text('Faster finger = faster cursor'),
              value: s.accel,
              onChanged: (v) => setState(() => s.accel = v),
            ),
            SwitchListTile(
              title: const Text('Natural scroll'),
              subtitle: const Text('Content follows your fingers'),
              value: s.natural,
              onChanged: (v) => setState(() => s.natural = v),
            ),
            SwitchListTile(
              title: const Text('Tap to click'),
              value: s.tapToClick,
              onChanged: (v) => setState(() => s.tapToClick = v),
            ),
            SwitchListTile(
              title: const Text('Drag lock'),
              subtitle: const Text('Stays held after a drag until next tap'),
              value: s.dragLock,
              onChanged: (v) => setState(() => s.dragLock = v),
            ),
            const SizedBox(height: 8),
            ElevatedButton(
              onPressed: widget.onChanged,
              child: const Text('Done'),
            ),
          ],
        ),
      ),
    );
  }
}
