# Phase 1B implementation review

Review target: `b4806d7` on `pwa-next`.

This review records code inspection and local build results only. It contains no
pairing codes, session tokens, LAN addresses, local paths, or device IDs.

## What is sound

- The QR nonce is generated with the existing random helper, is memory-only,
  expires after two minutes, and is cleared after a successful pairing or logout.
- The QR issuance endpoint checks that its direct caller is loopback-only.
- The existing manual pairing code remains accepted by `POST /pair`.
- The PWA keeps the session token in memory and persists only Host/Port.
- Service Worker caching remains limited to the Pages-origin static shell; LAN
  HTTP and WebSocket traffic are not handled by the Service Worker.
- CORS, WebSocket Origin handling, and CDP loopback binding were not relaxed by
  this change.

## Verification performed

- `cargo check`: passed.
- `cargo test`: passed (three tests, including QR text generation).
- `cargo build --release`: passed.
- `git diff --check`: passed.
- `cargo fmt --check`: **failed**. The committed Rust code needs formatting.

## Fix before physical-device validation

### 1. Validate QR connection payloads before network access — high priority

`pwa-poc/index.html` accepts any truthy `host`, `port`, and `nonce` from a
decoded QR and immediately calls the Host. A malicious QR can therefore make a
camera-scanning client attempt requests to arbitrary addresses after Local
Network Access has been granted.

Validate the decoded payload before `doConnect`:

- exact protocol version;
- an IPv4 private or link-local Host address only;
- an integer port from 1 through 65535;
- a nonce matching the Host-issued format (32 lower-case hex characters).

On failure, keep scanning and show the existing invalid-code message. Do not
silently fall back to a different address.

### 2. Format and re-run the Rust checks — high priority

Run `cargo fmt`, then re-run `cargo fmt --check`, `cargo check`, `cargo test`,
`cargo build --release`, and `git diff --check` before the next commit.

### 3. Correct the status document — medium priority

`docs/pwa-next-status.md` still says QR is deferred / not to be added, while
`b4806d7` adds QR issuance and scanning. Update it to describe the actual Phase
1B state and clearly label physical-device validation as pending.

### 4. Add QR lifecycle coverage — medium priority

The current QR unit test only confirms that ASCII QR text renders. Add focused
coverage for nonce expiry, one-time consumption, manual-code compatibility, and
loopback-only QR issuance. The tests must not log secrets.

### 5. Preserve third-party attribution — medium priority

`pwa-poc/jsQR.min.js` is a bundled third-party dependency. Add its license and
source/version attribution in an appropriate notice file before distribution.

## Follow-up validation after the fixes

1. Scan a valid newly generated QR and pair successfully.
2. Scan an expired QR and confirm rejection.
3. Confirm a previously consumed QR cannot pair a second time.
4. Confirm malformed and non-private-target QR payloads do not trigger LAN
   connection attempts.
5. Re-run manual Host/Port/pairing as a compatibility check.
6. On a physical device, verify install, standalone launch, camera permission,
   Local Network Access, control/frame WebSockets, JPEG display, reload, logout,
   and reconnect.

## Non-blocking architecture note

The new Pages client and the embedded local client are currently separate HTML
implementations. This is acceptable for the present experiment, but future UI
changes must be tested in both clients until shared client code is deliberately
introduced.

## Working tree note

An untracked `patch.py` exists at review time. It is not part of `b4806d7` and
was intentionally not deleted or committed. Review its ownership before any
cleanup commit.
