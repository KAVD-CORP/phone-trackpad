import 'package:flutter/material.dart';

import 'keyboard_screen.dart';
import 'net.dart';
import 'session.dart';
import 'trackpad_screen.dart';

/// Shown right after pairing or reconnect (ADR-0005): the user picks what
/// this session will control. Both modes ride the same live session, so
/// switching later is free — this is only the starting point.
class ModeSelectScreen extends StatefulWidget {
  final SessionManager sessions;
  final LiveSession session;

  const ModeSelectScreen({super.key, required this.sessions, required this.session});

  @override
  State<ModeSelectScreen> createState() => _ModeSelectScreenState();
}

class _ModeSelectScreenState extends State<ModeSelectScreen> {
  final SecureSender _sender = SecureSender();
  String _status = 'Connecting…';

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
        .then((_) {
      if (mounted) setState(() => _status = 'Connected to ${s.computer.name}');
    }).catchError((Object e) {
      if (mounted) {
        setState(() => _status = 'Could not reach ${s.computer.name} ($e)');
      }
    });
  }

  @override
  void dispose() {
    _sender.close();
    super.dispose();
  }

  void _open(Widget screen) {
    if (!_sender.isReady) return;
    Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => screen),
    );
  }

  Future<void> _disconnect() async {
    widget.sessions.disconnected();
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.session.computer.name),
        actions: [
          IconButton(
            icon: const Icon(Icons.link_off),
            tooltip: 'Disconnect',
            onPressed: _disconnect,
          ),
        ],
      ),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(_status, semanticsLabel: 'Connection status'),
            const SizedBox(height: 16),
            Text('What should this phone control?',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 16),
            _ModeCard(
              icon: Icons.touch_app,
              title: 'Trackpad',
              subtitle: 'Move the cursor, click, drag, scroll',
              onTap: () => _open(TrackpadScreen(
                sessions: widget.sessions,
                session: widget.session,
                sender: _sender,
                status: _status,
              )),
            ),
            const SizedBox(height: 12),
            _ModeCard(
              icon: Icons.keyboard,
              title: 'Keyboard',
              subtitle: 'Type text and special keys on the computer',
              onTap: () => _open(KeyboardScreen(
                session: widget.session,
                sender: _sender,
              )),
            ),
          ],
        ),
      ),
    );
  }
}

class _ModeCard extends StatelessWidget {
  final IconData icon;
  final String title;
  final String subtitle;
  final VoidCallback onTap;

  const _ModeCard({
    required this.icon,
    required this.title,
    required this.subtitle,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return Card(
      child: ListTile(
        leading: Icon(icon, size: 32),
        title: Text(title, style: Theme.of(context).textTheme.titleLarge),
        subtitle: Text(subtitle),
        trailing: const Icon(Icons.chevron_right),
        onTap: onTap,
      ),
    );
  }
}
