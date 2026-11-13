import 'dart:async';

import 'package:flutter/material.dart';

import 'gesture.dart';
import 'mode_select_screen.dart';
import 'net.dart';
import 'session.dart';

/// Phase 4 trackpad screen: gesture engine → sealed UDP sender. Shown from
/// the mode selector, which owns and connects the shared sender.
class TrackpadScreen extends StatefulWidget {
  final SessionManager sessions;
  final LiveSession session;
  final SecureSender sender;
  final String status;

  const TrackpadScreen({
    super.key,
    required this.sessions,
    required this.session,
    required this.sender,
    this.status = 'Connecting…',
  });

  @override
  State<TrackpadScreen> createState() => _TrackpadScreenState();
}

class _TrackpadScreenState extends State<TrackpadScreen> {
  final _settings = TrackpadSettings();
  late GestureEngine _engine = GestureEngine(_settings);
  late String _status = widget.status;
  Timer? _holdTimer;

  @override
  void initState() {
    super.initState();
    setState(() => _status = widget.status);
  }

  @override
  void dispose() {
    _holdTimer?.cancel();
    super.dispose();
  }

  void _emit(List<GestureOut> outs) {
    if (!widget.sender.isReady) return;
    for (final o in outs) {
      switch (o) {
        case OutMove(dx: final dx, dy: final dy):
          widget.sender.sendMove(dx.toDouble(), dy.toDouble());
        case OutScroll(dx: final dx, dy: final dy):
          widget.sender.sendScroll(dx, dy);
        case OutClick(button: final b):
          widget.sender.sendClick(b);
        case OutDown(button: final b):
          widget.sender.sendButton(b, true);
        case OutUp(button: final b):
          widget.sender.sendButton(b, false);
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
            icon: const Icon(Icons.keyboard),
            tooltip: 'Switch to keyboard',
            onPressed: () => Navigator.of(context).pop(),
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
