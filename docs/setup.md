# Setup & troubleshooting (Windows + iPhone)

## No-install path: iPhone browser (fastest)

No Mac, no App Store, no sideload needed:

1. On the computer: Phone Trackpad → **No app? Use the browser** →
   **Show browser code**. Note the address (`http://192.168.x.x:51517`)
   and the 6-digit code.
2. On the iPhone (same Wi-Fi): open **Safari**, type the address.
   Tip: Share → **Add to Home Screen** for a full-screen icon.
3. Type the 6-digit code on the page. The trackpad surface appears.
4. Use it: drag to move, tap to click, two fingers for right-click and
   scroll, tap-hold-drag for dragging. Keep Safari in the foreground.
5. Only on trusted Wi-Fi: browser mode is PIN-gated but not encrypted
   (the installed app encrypts everything).

## Installed-app path (first-time setup, 5 minutes)

1. On the computer: run the desktop app (or `trackpad-service --pair`).
   Approve the Windows Firewall prompt for **private** networks.
2. Click **Show pairing code**. A QR code and a 6-digit code appear.
3. On the iPhone (same Wi-Fi): open the app → **Scan pairing code** →
   point at the QR. Allow the Local Network permission when asked.
4. On the computer: click **Allow** next to your phone's name.
5. The phone is now a trackpad. It reconnects automatically afterwards.

No account, no cloud, nothing leaves your Wi-Fi.

## No camera / code won't scan

Pick the computer in the **On this Wi-Fi** list, tap it, and type the
6-digit code shown on the computer. (Needs discovery; see below.)

## Computer doesn't appear in the list

- Same Wi-Fi on both devices (guest networks often block discovery —
  the QR scan still works there).
- Firewall: the app needs UDP 51515 + TCP 51516 on private networks.
  Re-run and accept the prompt, or add the rule manually for
  `trackpad-service.exe` / the desktop app.
- VPNs and "public network" profiles block LAN traffic: switch the Wi-Fi
  profile to Private (Settings → Network → Wi-Fi → network → Private).

## "Could not reach" / pairing fails

- The code expires after ~2 minutes: tap **Show pairing code** again.
- Three wrong codes lock pairing for 30 seconds: wait, then retry.
- iPhone: Settings → the app → **Local Network** must be on. Without it,
  iOS silently drops LAN traffic.

## Cursor doesn't move but it says connected

- Reconnect from the app's start screen ( Wi-Fi blips are handled, but a
  changed IP address needs one manual reconnect).
- On the computer, check the app shows your phone under Trusted phones.
  If it was revoked, pair again.

## Revoking a phone

Desktop app → Trusted phones → **Revoke**. Takes effect on the next
packet (milliseconds while in use, instantly when idle since there is
nothing to stop). The phone keeps its stored keys but the computer will
no longer answer it — pair again to re-trust.

## Advanced: manual address

If both discovery and QR fail (strict corporate Wi-Fi), enter the
computer's address by hand: on the computer run `ipconfig` and use the
`192.168.x.x` address. Short codes still need discovery for the server
fingerprint; the QR path works fully offline.
