import 'package:flutter/material.dart';

import 'hid.dart';
import 'net.dart';
import 'session.dart';

/// Keyboard mode (ADR-0005): the phone's soft keyboard types TEXT chunks,
/// and a fixed row sends the keys a soft keyboard cannot emit (Esc, Tab,
/// arrows, Home/End, PageUp/PageDown, Del, F1–F12). Sticky modifiers ride
/// the `modifiers` bitmask of every KEY event until toggled off.
class KeyboardScreen extends StatefulWidget {
  final LiveSession session;
  final SecureSender sender;

  const KeyboardScreen({super.key, required this.session, required this.sender});

  @override
  State<KeyboardScreen> createState() => _KeyboardScreenState();
}

class _KeyboardScreenState extends State<KeyboardScreen> {
  final _controller = TextEditingController();
  final _focus = FocusNode();
  int _modifiers = 0;
  String _last = '';

  bool get _ready => widget.sender.isReady;

  /// Send one tap of a special key: down + redundant up, both carrying the
  /// sticky modifier bitmask so the desktop presses/releases the modifiers
  /// around it. The desktop's KeyGuard releases anything stuck anyway.
  void _sendKey(int usage) {
    if (!_ready) return;
    widget.sender.sendKey(usage, _modifiers, true);
    widget.sender.sendKey(usage, _modifiers, false);
  }

  void _toggleModifier(int bit) {
    setState(() => _modifiers ^= bit);
  }

  /// Soft-keyboard delta → TEXT chunks / Backspace KEYs. The committed
  /// text is what the desktop receives, not per-keystroke noise.
  void _onChanged(String text) {
    if (!_ready) {
      _last = text;
      return;
    }
    if (text.length > _last.length && text.startsWith(_last)) {
      final added = text.substring(_last.length);
      for (var i = 0; i < added.length; i += maxTextChunk) {
        final end = (i + maxTextChunk).clamp(0, added.length);
        widget.sender.sendText(added.substring(i, end));
      }
    } else if (text.length < _last.length && _last.startsWith(text)) {
      final removed = _last.length - text.length;
      for (var i = 0; i < removed; i++) {
        widget.sender.sendKey(Hid.backspace, 0, true);
        widget.sender.sendKey(Hid.backspace, 0, false);
      }
    } else {
      // Cursor moved or a selection was replaced: backspace the old text
      // and type the new (best effort; uncommon for plain typing).
      for (var i = 0; i < _last.length; i++) {
        widget.sender.sendKey(Hid.backspace, 0, true);
        widget.sender.sendKey(Hid.backspace, 0, false);
      }
      for (var i = 0; i < text.length; i += maxTextChunk) {
        final end = (i + maxTextChunk).clamp(0, text.length);
        widget.sender.sendText(text.substring(i, end));
      }
    }
    _last = text;
  }

  @override
  void dispose() {
    // Release any sticky modifiers so the desktop is left clean.
    for (final (bit, usage) in _modifierBits) {
      if (_modifiers & bit != 0) {
        widget.sender.sendKey(usage, 0, true);
        widget.sender.sendKey(usage, 0, false);
      }
    }
    _controller.dispose();
    _focus.dispose();
    super.dispose();
  }

  static const _modifierBits = [
    (Hid.modCtrl, _ctrlUsage),
    (Hid.modAlt, _altUsage),
    (Hid.modShift, _shiftUsage),
    (Hid.modMeta, _metaUsage),
  ];
  static const int _ctrlUsage = 0xe0; // Keyboard LeftControl
  static const int _shiftUsage = 0xe1; // Keyboard LeftShift
  static const int _altUsage = 0xe2; // Keyboard LeftAlt
  static const int _metaUsage = 0xe3; // Keyboard LeftGUI

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Keyboard'),
        actions: [
          IconButton(
            icon: const Icon(Icons.touch_app),
            tooltip: 'Switch to trackpad',
            onPressed: () => Navigator.of(context).pop(),
          ),
        ],
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
            child: Text(
              _ready ? 'Typing on ${widget.session.computer.name}' : 'Connecting…',
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
            child: Wrap(
              spacing: 6,
              children: [
                for (final (bit, label) in const [
                  (Hid.modCtrl, 'Ctrl'),
                  (Hid.modAlt, 'Alt'),
                  (Hid.modShift, 'Shift'),
                  (Hid.modMeta, 'Meta'),
                ])
                  FilterChip(
                    label: Text(label),
                    selected: _modifiers & bit != 0,
                    onSelected: (_) => _toggleModifier(bit),
                  ),
              ],
            ),
          ),
          Expanded(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: TextField(
                controller: _controller,
                focusNode: _focus,
                autofocus: true,
                maxLines: null,
                expands: true,
                textAlignVertical: TextAlignVertical.top,
                onChanged: _onChanged,
                decoration: const InputDecoration(
                  hintText: 'Type here — text lands on the computer',
                  border: OutlineInputBorder(),
                ),
              ),
            ),
          ),
          _KeyRow(
            keys: const [
              ('Esc', Hid.escape),
              ('Tab', Hid.tab),
              ('←', Hid.arrowLeft),
              ('↑', Hid.arrowUp),
              ('↓', Hid.arrowDown),
              ('→', Hid.arrowRight),
            ],
            onTap: _sendKey,
          ),
          _KeyRow(
            keys: const [
              ('Home', Hid.home),
              ('End', Hid.end),
              ('PgUp', Hid.pageUp),
              ('PgDn', Hid.pageDown),
              ('Del', Hid.delete),
            ],
            onTap: _sendKey,
          ),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
            child: Row(
              children: [
                for (var n = 1; n <= 12; n++)
                  Padding(
                    padding: const EdgeInsets.only(right: 6),
                    child: OutlinedButton(
                      onPressed: () => _sendKey(Hid.fKey(n)),
                      child: Text('F$n'),
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

typedef _KeySpec = (String, int);

class _KeyRow extends StatelessWidget {
  final List<_KeySpec> keys;
  final void Function(int usage) onTap;

  const _KeyRow({required this.keys, required this.onTap});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      child: Row(
        children: [
          for (final (label, usage) in keys)
            Expanded(
              child: Padding(
                padding: const EdgeInsets.only(right: 6),
                child: OutlinedButton(
                  onPressed: () => onTap(usage),
                  child: Text(label),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
