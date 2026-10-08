import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import 'discovery.dart';
import 'session.dart';
import 'trackpad_screen.dart';
import 'src/rust/api.dart/frb_generated.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  final sessions = SessionManager();
  await sessions.init();
  runApp(TrackpadApp(sessions: sessions));
}

class TrackpadApp extends StatelessWidget {
  final SessionManager sessions;

  const TrackpadApp({super.key, required this.sessions});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Phone Trackpad',
      theme: ThemeData(colorSchemeSeed: Colors.blue, useMaterial3: true),
      darkTheme: ThemeData.dark(useMaterial3: true),
      home: sessions.isPaired
          ? ReconnectScreen(sessions: sessions)
          : OnboardingScreen(sessions: sessions),
    );
  }
}

/// Known computer stored: one-tap reconnect, or pair a new one.
class ReconnectScreen extends StatefulWidget {
  final SessionManager sessions;

  const ReconnectScreen({super.key, required this.sessions});

  @override
  State<ReconnectScreen> createState() => _ReconnectScreenState();
}

class _ReconnectScreenState extends State<ReconnectScreen> {
  String _status = '';

  Future<void> _reconnect() async {
    setState(() => _status = 'Reconnecting…');
    try {
      final session = await widget.sessions.reconnect();
      if (!mounted) return;
      await Navigator.of(context).push(
        MaterialPageRoute(
          builder: (_) => TrackpadScreen(sessions: widget.sessions, session: session),
        ),
      );
      setState(() => _status = '');
    } catch (e) {
      setState(() => _status = 'Could not reconnect ($e)');
    }
  }

  @override
  Widget build(BuildContext context) {
    final name = widget.sessions.computer?.name ?? 'computer';
    return Scaffold(
      appBar: AppBar(title: const Text('Phone Trackpad')),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text('Last used: $name', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 12),
            ElevatedButton(onPressed: _reconnect, child: const Text('Connect')),
            TextButton(
              onPressed: () => Navigator.of(context).push(
                MaterialPageRoute(
                  builder: (_) => OnboardingScreen(sessions: widget.sessions),
                ),
              ),
              child: const Text('Pair a different computer'),
            ),
            TextButton(
              onPressed: () async {
                await widget.sessions.forget();
                if (context.mounted) {
                  Navigator.of(context).pushAndRemoveUntil(
                    MaterialPageRoute(
                      builder: (_) => OnboardingScreen(sessions: widget.sessions),
                    ),
                    (_) => false,
                  );
                }
              },
              child: const Text('Forget this computer'),
            ),
            if (_status.isNotEmpty) Text(_status),
          ],
        ),
      ),
    );
  }
}

/// First run: discovered computers, QR scan, short code.
class OnboardingScreen extends StatefulWidget {
  final SessionManager sessions;
  final Future<List<FoundComputer>> Function() browse;

  const OnboardingScreen({super.key, required this.sessions, this.browse = browseComputers});

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends State<OnboardingScreen> {
  List<FoundComputer> _found = [];
  bool _scanning = true;

  @override
  void initState() {
    super.initState();
    _browse();
  }

  Future<void> _browse() async {
    setState(() => _scanning = true);
    final found = await widget.browse();
    if (mounted) {
      setState(() {
        _found = found;
        _scanning = false;
      });
    }
  }

  Future<void> _openTrackpad(LiveSession session) async {
    if (!mounted) return;
    await Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => TrackpadScreen(sessions: widget.sessions, session: session),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Find your computer')),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ElevatedButton.icon(
              icon: const Icon(Icons.qr_code_scanner),
              label: const Text('Scan pairing code'),
              onPressed: () => Navigator.of(context).push(
                MaterialPageRoute(
                  builder: (_) => ScanScreen(sessions: widget.sessions, onPaired: _openTrackpad),
                ),
              ),
            ),
            const SizedBox(height: 16),
            Text('On this Wi-Fi', style: Theme.of(context).textTheme.titleMedium),
            if (_scanning)
              const Padding(
                padding: EdgeInsets.all(12),
                child: Center(child: CircularProgressIndicator()),
              )
            else if (_found.isEmpty)
              const Text('Nothing found. The code scan works even when discovery is blocked.'),
            for (final c in _found)
              ListTile(
                title: Text(c.name),
                subtitle: const Text('Tap to enter the short code'),
                trailing: const Icon(Icons.chevron_right),
                onTap: () => Navigator.of(context).push(
                  MaterialPageRoute(
                    builder: (_) => CodeScreen(
                      sessions: widget.sessions,
                      computer: c,
                      onPaired: _openTrackpad,
                    ),
                  ),
                ),
              ),
            TextButton.icon(
              icon: const Icon(Icons.refresh),
              label: const Text('Scan again'),
              onPressed: _browse,
            ),
          ],
        ),
      ),
    );
  }
}

/// QR scan → pair (blocks on desktop approval) → trackpad.
class ScanScreen extends StatefulWidget {
  final SessionManager sessions;
  final Future<void> Function(LiveSession) onPaired;

  const ScanScreen({super.key, required this.sessions, required this.onPaired});

  @override
  State<ScanScreen> createState() => _ScanScreenState();
}

class _ScanScreenState extends State<ScanScreen> {
  final _controller = MobileScannerController();
  String _status = 'Point the camera at the code on your computer.';
  bool _busy = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _pair(String raw) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _status = 'Code scanned. Approve on the computer…';
    });
    try {
      // QR carries the address; the manual host is only a fallback.
      final session = await widget.sessions.pairWithQr(raw, '');
      await widget.onPaired(session);
    } catch (e) {
      setState(() {
        _busy = false;
        _status = 'Pairing failed ($e). Try again.';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Scan pairing code')),
      body: Column(
        children: [
          Expanded(
            child: MobileScanner(
              controller: _controller,
              onDetect: (capture) {
                if (capture.barcodes.isEmpty) return;
                final v = capture.barcodes.first.rawValue;
                if (v != null) _pair(v);
              },
              errorBuilder: (context, error) => Center(
                child: Padding(
                  padding: const EdgeInsets.all(24),
                  child: Text(
                    'Camera unavailable ($error). Allow camera access for this app, or enter the 6-digit code instead.',
                    textAlign: TextAlign.center,
                  ),
                ),
              ),
            ),
          ),
          Padding(padding: const EdgeInsets.all(16), child: Text(_status)),
        ],
      ),
    );
  }
}

/// Short-code entry for a discovered (or manual) computer.
class CodeScreen extends StatefulWidget {
  final SessionManager sessions;
  final FoundComputer? computer;
  final Future<void> Function(LiveSession) onPaired;

  const CodeScreen({super.key, required this.sessions, this.computer, required this.onPaired});

  @override
  State<CodeScreen> createState() => _CodeScreenState();
}

class _CodeScreenState extends State<CodeScreen> {
  final _host = TextEditingController();
  final _tcp = TextEditingController(text: '51516');
  final _udp = TextEditingController(text: '51515');
  final _code = TextEditingController();
  String _status = '';
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    final c = widget.computer;
    if (c != null) {
      _host.text = c.host;
      _tcp.text = '${c.tcpPort}';
      _udp.text = '${c.udpPort}';
    }
  }

  @override
  void dispose() {
    _host.dispose();
    _tcp.dispose();
    _udp.dispose();
    _code.dispose();
    super.dispose();
  }

  Future<void> _pair() async {
    final c = widget.computer;
    final fp = c?.fingerprint;
    if (c == null || fp == null) {
      // No fingerprint source (mDNS blocked or manual entry): the QR
      // carries it instead.
      setState(() {
        _status = 'Pick a discovered computer, or scan the QR — it carries everything needed.';
      });
      return;
    }
    setState(() {
      _busy = true;
      _status = 'Exchanging codes. Approve on the computer…';
    });
    try {
      final session = await widget.sessions.pairWithCode(
        host: _host.text.trim().isEmpty ? c.host : _host.text.trim(),
        tcpPort: int.tryParse(_tcp.text.trim()) ?? c.tcpPort,
        udpPort: int.tryParse(_udp.text.trim()) ?? c.udpPort,
        code: _code.text.trim(),
        serverFp: fp,
        serverName: c.name,
      );
      await widget.onPaired(session);
    } catch (e) {
      setState(() {
        _busy = false;
        _status = 'Pairing failed ($e). Check the code and try again.';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Enter short code')),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(controller: _host, decoration: const InputDecoration(labelText: 'Computer address')),
            TextField(controller: _code, decoration: const InputDecoration(labelText: '6-digit code'), keyboardType: TextInputType.number),
            const SizedBox(height: 12),
            ElevatedButton(onPressed: _busy ? null : _pair, child: const Text('Pair')),
            if (_status.isNotEmpty) Text(_status),
          ],
        ),
      ),
    );
  }
}
