# ADR-0001 — Flutter for the phone

Status: Accepted (6 Oct 2026)

The phone is a full-screen trackpad for iOS and Android. Flutter draws that once and already owns gestures well enough that a second native UI is not justified. React Native stays off this path for the same reason it stayed off Phone Controller: the screen is not a form, and the pointer path should not cross a JavaScript bridge. Native code is allowed later as a platform channel if a gesture API is missing, not as a second app.
