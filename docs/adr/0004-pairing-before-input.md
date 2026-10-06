# ADR-0004 — No input without a paired session

Status: Accepted (6 Oct 2026)

The desktop agent can move the user's cursor. That is the product, and it is also the thing that must not be open to the rest of the LAN.

Local use does not need an account or a cloud. It does need a session. The phone pairs with a QR code or a short code. The desktop assigns a non-zero session id. Motion and control for any other id are dropped. The user can revoke a trusted device. The menu bar shows a connected state for the whole session.

A MAC over the session secret is required before a packaged release (Phase 4). The bootstrap codec does not pretend that check already exists. Do not add a silent background mode, and do not skip the macOS Accessibility prompt.
