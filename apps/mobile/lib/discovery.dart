import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:multicast_dns/multicast_dns.dart';

/// A computer found on the LAN via mDNS.
class FoundComputer {
  final String name;
  final InternetAddress address;
  final int tcpPort;
  final int udpPort;

  /// Server fingerprint from the TXT record, when advertised. Short-code
  /// pairing needs it (binds the code to the desktop); without it the user
  /// must scan the QR instead.
  final Uint8List? fingerprint;

  const FoundComputer({
    required this.name,
    required this.address,
    required this.tcpPort,
    required this.udpPort,
    this.fingerprint,
  });

  String get host => address.address;
}

/// Browse `_wptrackpad._tcp.local` for up to [timeout]. Returns whatever
/// answered in time (possibly empty — mDNS is often blocked on guest Wi-Fi,
/// in which case the QR carries the address instead).
Future<List<FoundComputer>> browseComputers({Duration timeout = const Duration(seconds: 5)}) async {
  final client = MDnsClient();
  try {
    await client.start();
  } catch (_) {
    client.stop();
    return const [];
  }
  final found = <String, FoundComputer>{};
  Future<void> run() async {
    await for (final ptr in client.lookup<PtrResourceRecord>(
      ResourceRecordQuery.serverPointer('_wptrackpad._tcp.local'),
    )) {
      SrpLoop:
      await for (final srv in client.lookup<SrvResourceRecord>(
        ResourceRecordQuery.service(ptr.domainName),
      )) {
        var udpPort = 51515;
        Uint8List? fingerprint;
        await for (final txt in client.lookup<TxtResourceRecord>(
          ResourceRecordQuery.text(ptr.domainName),
        )) {
          for (final s in txt.text.split('\n')) {
            if (s.startsWith('udp=')) {
              udpPort = int.tryParse(s.substring(4)) ?? udpPort;
            } else if (s.startsWith('fp=')) {
              try {
                final raw = base64Url.decode(base64.normalize(s.substring(3)));
                if (raw.length == 32) fingerprint = Uint8List.fromList(raw);
              } catch (_) {
                fingerprint = null;
              }
            }
          }
          break;
        }
        await for (final ip in client.lookup<IPAddressResourceRecord>(
          ResourceRecordQuery.addressIPv4(srv.target),
        )) {
          if (ip.address.isLoopback) continue;
          final name = ptr.domainName.split('.').first;
          found[name] = FoundComputer(
            name: name,
            address: ip.address,
            tcpPort: srv.port,
            udpPort: udpPort,
            fingerprint: fingerprint,
          );
          break SrpLoop;
        }
      }
    }
  }

  try {
    await run().timeout(timeout);
  } catch (_) {
    // Timeout or lookup errors just end the browse with what we have.
  }
  client.stop();
  return found.values.toList();
}
