# PWA next status

Last updated: 2026-09-02

This document tracks the experimental PWA work on `pwa-next`. It deliberately
contains no pairing codes, session tokens, LAN addresses, local paths, device
identifiers, or account information.

## Branches

- Stable baseline: `main` / `v0.1.8` at `84594c9` — unchanged.
- PWA development: `pwa-next`.
- No `v0.2.0` release, tag, or merge to `main` has been made.

## Existing product baseline

The stable host has previously passed its core milestones:

- Headless Brave, CDP input, and PC-side audio.
- JPEG viewer, tabs, pairing/authentication, logout, and local Web client.
- CDP loopback isolation and Host security regressions.

The embedded local client remains the fallback while PWA work is experimental.

## Phase 0: Pages-to-LAN feasibility — passed

The minimal GitHub Pages client established that an HTTPS Pages origin can use
Android Chrome Local Network Access to reach the LAN Host. The following were
observed in Android Chrome:

- Pages HTTPS and secure context.
- Local Network Access permission prompt and successful Host health request.
- Pairing with existing Host authentication.
- Authenticated control and frame WebSockets.
- JPEG decoding/display and a reload command.

Host protections retained on `pwa-next`:

- CORS is limited to the Pages origin; no wildcard CORS.
- WebSockets accept the Pages origin or the same-host local client; other
  origins are rejected.
- CDP remains loopback-only.
- Pairing responses no longer show session tokens in the PWA diagnostic log.

## Phase 1A: installable PWA shell — in progress

Implemented on `pwa-next`:

- `manifest.webmanifest` with standalone display metadata and an app icon.
- A Service Worker that caches only the GitHub Pages static shell.
- Cache versioning and removal of obsolete shell caches.
- LAN Host HTTP and WebSocket traffic is outside the Service Worker origin and
  is not cached.
- A diagnostic camera preview with an explicit Stop button; it displays the
  negotiated video dimensions and releases all tracks when stopped.

Recent commits:

- `76a5b40` — initial installable shell.
- `42d4581` — obsolete shell-cache cleanup.
- `6ea6f13` — visible camera preview and explicit cleanup.

## Android validation status

Confirmed:

- Emulator boot completed.
- The emulator camera configuration was repaired from zero visible camera
  devices to one device. It was then switched to the official Virtual Scene
  camera mode so a PNG/JPEG can be used as camera content.
- In a regular Chrome tab, the Pages client reported a secure context and
  registered its Service Worker under the Pages subpath.

Still to validate manually in the emulator:

1. Add a PNG/JPEG under Extended Controls → Camera → Virtual scene images.
2. Open the Pages client and use **Start Camera Preview**; allow camera access,
   verify the preview/dimensions, then use **Stop Camera**.
3. Install the PWA through Chrome's normal install UI, launch it from the
   launcher, and verify standalone display mode.
4. In standalone mode, re-test camera preview, Local Network Access, Host
   health, pairing, authenticated control/frame WebSockets, JPEG display,
   reload, and reconnect after closing/reopening only the PWA.
5. Re-run invalid-token and wrong-Origin checks against the final Host run.

## Known test note

The historical tab regression script can fail when its public YouTube test
video pauses shortly after startup. This was observed independently of the PWA
and CORS changes; it is not yet classified as a PWA regression.

## Decision gates

- Phase 1A is complete when installed standalone PWA operation is confirmed for
  Local Network Access, pairing, control/frame WebSockets, JPEG display, reload,
  reconnect, and retained security.
- Phase 1B (migrating the established client UI) may start after the PWA runtime
  gate passes, even if emulator-only camera testing remains unavailable.
- QR discovery/pairing is explicitly deferred until camera preview is validated.

## Operating rules

- Do not weaken pairing/authentication, Origin validation, CORS restrictions, or
  CDP loopback isolation for PWA work.
- Do not add QR, host discovery, cloud relay, HTTPS on the Rust Host, or a
  `v0.2.0` release in the current phase.
- Keep `main` and released `v0.1.x` artifacts immutable.
