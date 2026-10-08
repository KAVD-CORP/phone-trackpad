# iPhone testing without a paid Apple Developer account

You have: iPhone 15 + a MacBook on the side. That is enough for device
testing via free sideloading (7-day certificates). Paid account needed
only for TestFlight / App Store later.

## One-time MacBook setup

1. Install **Xcode** from the Mac App Store, open it once (accept license),
   install additional components when prompted.
2. Install Flutter **3.35.4** (matches `.flutter-version`):
   download `flutter_macos_3.35.4-stable.zip` from the Flutter site,
   unzip to `~/flutter`, add `~/flutter/bin` to PATH.
3. Xcode → Settings → Accounts → add your free Apple ID.
4. On the iPhone: Settings → Privacy & Security → **Developer Mode** → on
   (iOS 16+ requires this for sideloaded apps). Connect via USB, tap Trust.

## Build + install (repeat weekly — free certs expire in 7 days)

```bash
cd apps/mobile
# ios/ is committed with pairing permissions pre-filled. On the MacBook:
flutter pub get
open ios/Runner.xcworkspace   # set your Team + unique Bundle ID, then run
```

In Xcode: Runner target → Signing & Capabilities → Team = your Apple ID.
Change the Bundle Identifier to something unique
(`com.wptrackpad.mobile.<yourname>` — free accounts can't use arbitrary
shared IDs). Then:

```bash
flutter run -d <your-iphone-id>   # flutter devices lists it
```

First launch: iPhone Settings → General → VPN & Device Management →
trust your Apple ID.

## The actual test

1. Windows PC + iPhone on the **same Wi-Fi**.
2. PC app → **Show pairing code**. iPhone app → **Scan pairing code**.
3. Allow camera + **Local Network** permission when iOS asks. Without
   Local Network, LAN traffic silently fails — this is the #1 gotcha.
4. Approve on the PC. Run `docs/test-plans/phase4-manual.md` steps 7–14.

## Limits of free sideloading

- Rebuild/reinstall every 7 days. Keep the MacBook flow handy.
- Max ~3 sideloaded apps per Apple ID.
- No TestFlight, no App Store, no push notifications (we use none).
- When ready for distribution: paid Apple Developer Program ($99/yr),
  then `flutter build ipa` + App Store Connect (notes in
  `docs/packaging.md`).
