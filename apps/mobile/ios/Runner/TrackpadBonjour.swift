import Foundation

/// Uses the system Bonjour service for this declared service type. No raw
/// multicast socket or restricted multicast entitlement is needed on iOS.
final class TrackpadBonjour: NSObject, NetServiceBrowserDelegate, NetServiceDelegate {
  private var browser: NetServiceBrowser?
  private var services: [NetService] = []
  private var found: [String: [String: Any]] = [:]
  private var timer: Timer?
  private var completion: (([[String: Any]]) -> Void)?

  func browse(timeout: TimeInterval, completion: @escaping ([[String: Any]]) -> Void) {
    finish()
    self.completion = completion
    found.removeAll()
    let browser = NetServiceBrowser()
    self.browser = browser
    browser.delegate = self
    browser.searchForServices(ofType: "_wptrackpad._tcp.", inDomain: "local.")
    timer = Timer.scheduledTimer(withTimeInterval: timeout, repeats: false) { [weak self] _ in
      self?.finish()
    }
  }

  func finish() {
    timer?.invalidate()
    timer = nil
    browser?.delegate = nil
    browser?.stop()
    browser = nil
    for service in services {
      service.delegate = nil
      service.stop()
    }
    services.removeAll()
    let callback = completion
    completion = nil
    callback?(Array(found.values))
  }

  func netServiceBrowser(_ browser: NetServiceBrowser, didFind service: NetService, moreComing: Bool) {
    services.append(service)
    service.delegate = self
    service.resolve(withTimeout: 4)
  }

  func netServiceBrowser(_ browser: NetServiceBrowser, didRemove service: NetService, moreComing: Bool) {
    found.removeValue(forKey: service.name)
    services.filter { $0 == service }.forEach { $0.stop() }
    services.removeAll { $0 == service }
  }

  func netServiceBrowser(_ browser: NetServiceBrowser, didNotSearch errorDict: [String: NSNumber]) {
    finish()
  }

  func netServiceDidResolveAddress(_ sender: NetService) {
    guard completion != nil, sender.port > 0, sender.port <= 65535 else { return }
    // The existing Windows transport and Dart sender use IPv4.
    let host = (sender.addresses ?? []).compactMap { data -> String? in
      data.withUnsafeBytes { bytes -> String? in
        guard let base = bytes.baseAddress, bytes.count >= MemoryLayout<sockaddr_in>.size else { return nil }
        let address = base.assumingMemoryBound(to: sockaddr.self)
        guard address.pointee.sa_family == sa_family_t(AF_INET) else { return nil }
        var buffer = [CChar](repeating: 0, count: Int(NI_MAXHOST))
        guard getnameinfo(address, socklen_t(data.count), &buffer,
                          socklen_t(buffer.count), nil, 0, NI_NUMERICHOST) == 0 else { return nil }
        return String(cString: buffer)
      }
    }.first
    guard let host = host, !host.hasPrefix("127.") else { return }
    let txt = sender.txtRecordData().map(NetService.dictionary(fromTXTRecord:)) ?? [:]
    func text(_ key: String) -> String? {
      txt[key].flatMap { String(data: $0, encoding: .utf8) }
    }
    let udp = text("udp").flatMap(Int.init) ?? 51515
    guard (1...65535).contains(udp) else { return }
    found[sender.name] = [
      "name": sender.name, "host": host, "tcpPort": sender.port,
      "udpPort": udp, "fingerprint": text("fp") ?? ""
    ]
  }
}
