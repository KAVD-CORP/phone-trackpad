import 'dart:async';

import 'package:flutter/material.dart';

import 'gesture.dart';
import 'net.dart';
import 'session.dart';

/// Phase 4 trackpad screen: gesture engine → sealed UDP sender.
class TrackpadScreen extends StatefulWidget {
  final SessionManager sessions;
  final LiveSession session;

  const TrackpadScreen({super.key, required this.sessions, required this.session});

  @override
  State<TrackpadScreen> createState() => _TrackpadScreenState();
}

class _TrackpadScreenState extends State<TrackpadScreen> {
  final _sender = SecureSender();
  final _settings = TrackpadSettings();
  late GestureEngine _engine = GestureEngine(_settings);
  String _status = 'Connecting…';
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
        .then((_) => setState(() => _status = 'Connected to ${s.computer.name}'))
        .catchError((Object e) {
      setState(() => _status = 'Could not reach ${s.computer.name} ($e)');
    });
  }

  @override
  void dispose() {
    _holdTimer?.cancel();
    _sender.close();
    super.dispose();
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
          Expanded(
            child: Listener(
              onPointerDown: (e) {
                _ensureHoldTimer();
                _emit(_engine.pointerDown(
                    e.pointer, e.position.dx, e.position.dy, _now()));
              },
              onPointerMove: (e) {
                _emit(_engine.pointerMove(
                    e.pointer, e.delta.dx, e.delta.dy, _now()));
              },
              onPointerUp: (e) {
                _emit(_engine.pointerUp(
                    e.pointer, e.position.dx, e.position.dy, _now()));
                _stopHoldTimer();
              },
              onPointerCancel: (e) {
                _emit(_engine.pointerUp(
                    e.pointer, e.position.dx, e.position.dy, _now()));
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
