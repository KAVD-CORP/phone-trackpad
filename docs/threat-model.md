# Threat model (Phase 4)

## What we protect

Unattended mouse control of the desktop. Anyone who can inject input can
install software, exfiltrate files, and impersonate the user. Pairing is
therefore authentication, and the default-deny posture is load-bearing:
**no trust, no input, ever.**

## Attackers considered

- **LAN neighbor / snoop:** sees all UDP/TCP bytes, injects arbitrary
  packets, replays captures, scans ports, spoofs source addresses.
- **QR shoulder-surfer:** photographs the desktop QR or reads the 6-digit
  code over the victim's shoulder during the 2-minute window.
- **Guessing attacker:** online short-code guessing against the TCP port.
- **Ex-phone:** a previously trusted phone after revocation (or a stolen
  unlocked phone — out of scope beyond revoke; see limits).
- **Malicious app on the desktop:** reads stored keys (mitigated by the OS
  keychain for the private key; trusted-peer public keys are public data).

Out of scope: attackers with desktop admin/root (game over by definition),
physical device theft while unlocked, RF out-of-LAN attackers (no cloud,
no port forwarding — the service binds LAN only).

## Protections (each with automated tests)

| Claim | Mechanism | Test |
|---|---|---|
| Only paired devices inject input | secure-only UDP default; v1 dropped + counted | `secure_untrusted_and_tampered_drop_silently`, live `--open`-off runs |
| Pairing needs the secret AND a human | QR secret/SPAKE2 PSK + explicit approve screen | `qr_pair_approve_and_reconnect`, `deny_leaves_no_trust_no_session` |
| Secrets expire, single-use | 2-min TTL enforced pre-crypto; consumed on approve | `expired_ceremony_rejects_before_crypto`, `secret_single_use_and_expiry`, `ceremony_consumed_on_approve` |
| Codes can't be brute-forced offline | SPAKE2 (one live guess per round) + 3-strikes/30 s lockout | `guard_locks_out_after_three_failures`, `wrong_secret_fails_and_lockout_kicks_in` |
| Wrong secret fails closed | Noise message-3 auth; initiators learn via missing OK | `wrong_psk_fails`, `spake2_same_code_agrees_wrong_code_fails` |
| MITM can't substitute keys | QR fingerprint check (phone), trust-store pin (reconnect) | fingerprint assert in `client_pair_qr`, `FingerprintMismatch` path |
| Replay/forgery impossible | AEAD + seq nonce + 1024-bit window + AAD-bound header | `tampered_ciphertext_and_header_rejected`, replay tests |
| Sessions are fresh | HKDF keys per handshake; rekey on reconnect | `pair_handshake_agrees_on_keys_and_statics` |
| Revoke is immediate | per-packet trust+live check; channel cleared, buttons released | `qr_pair_approve_and_reconnect` (revoke tail), `set_and_clear_secure_release_held_buttons` |
| Stuck input impossible | silence timeout + session-change release, encrypted or not | live `stuck` test, `silence_timeout_releases_held_buttons` |
| Pairing mode off by default | no ceremony exists until the user opens pairing; attempts fail | `active_secret` returns `Expired` with no ceremony |

## Browser mode (no install): accepted weaker properties

Safari → `http://<pc>:51517` → 6-digit PIN → JSON input. Deliberate
tradeoffs, shown in the UI ("Trusted Wi-Fi only"):

- **LAN-plaintext:** no encryption. A LAN eavesdropper sees cursor moves
  (not keys — there is no keyboard in the MVP). The native app stays the
  recommendation for untrusted networks.
- **PIN, not PAKE:** a 6-digit PIN is weak, but each guess needs a live
  server round and the server-wide 3-strikes/30 s lockout applies. The
  PIN is random per ceremony (10-min TTL), single-session (a new code
  revokes the old), and wrong guesses give no oracle (identical answers).
- **Same input safety:** batches flow through the shared session machine
  (coalescing, flush-before-button) with a 100 ms reaper enforcing the
  silence release if the browser dies.
- **DoS caps:** 8 KiB heads, 64 KiB bodies, 64 msgs/batch, per-step
  timeouts; oversize bodies kill the connection unread.
- The web server binds LAN-only alongside the main service (same
  firewall prompt covers it).

| Claim | Test |
|---|---|
| Wrong PIN fails, lockout after 3 | `pin_wrong_locks_out` |
| Expiry + single session | `pin_expires_and_single_session` |
| Bad tokens/JSON dropped, valid still apply | `input_applies_and_validates` |
| Full HTTP framing incl. oversize kill | `http_end_to_end` (real sockets) |

## Known limits (accepted, documented)

- A shoulder-surfed QR **within its 2-minute window** plus a fast approval
  race still needs the victim to approve the wrong device name — the
  approval screen showing the *phone-supplied* name is the last defense.
  Short codes have the same property with a smaller window (rate-limited).
- mDNS advertises the computer name + fingerprint. Fingerprints are public
  by design (TOFU binding), names leak presence. No addresses beyond LAN.
- Device names are peer-supplied and sanitized, never trusted for decisions.
- The desktop private key in the OS keychain is only as safe as the login.
- No forward secrecy across sessions beyond rekeying per handshake (Noise
  provides FS within the handshake; stored statics are long-term by design).
- Logs never contain keys, secrets, or codes (only truncated fingerprints
  in hex for revoke UX).
