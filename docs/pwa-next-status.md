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

## Phase 1A/1B: installable PWA and QR pairing — implementation complete, device validation pending

Implemented on `pwa-next`:

- `manifest.webmanifest` with standalone display metadata and an app icon.
- A Service Worker that caches only the GitHub Pages static shell (v2).
- Cache versioning and removal of obsolete shell caches.
- LAN Host HTTP and WebSocket traffic is outside the Service Worker origin and
  is not cached.

### Production PWA client (`pwa-poc/index.html`)

The PoC test UI has been replaced with a production-quality PWA client
that ports all features from the stable embedded `web/index.html`:

- Connect screen: Host IP, Port, Pairing Code input.
- Saved last-used Host IP/Port in `localStorage` (token never persisted).
- `targetAddressSpace: 'local'` on every fetch to the LAN Host (Local Network
  Access).
- Dynamic `ws://HOST:PORT/ws/control` and `ws://HOST:PORT/ws/frame` URLs.
- Full JPEG frame viewer with `object-fit: fill` in a `100dvh` grid layout.
- Touch tap mapped to remote JPEG coordinates.
- Touch scroll / swipe (accidental-tap prevention: moved flag).
- Back, Forward, Reload navigation via existing CDP history implementation.
- Tab list overlay.
- URL / Search bar.
- Text bridge with IME composition handling (no Windows key injection).
- Logout with token revocation and return to connect screen.
- Viewport resize propagation to Brave via `resize` control message.
- Session expiry detection on WebSocket close.
- `safe-area-inset-*` and `100dvh` mobile layout.
- Service Worker registration (static shell only).
- QR scanner uses the locally bundled jsQR 1.4.0 library; camera streams are
  stopped on success, cancel, and permission failure.
- QR payloads accept only HWMR version 1, private IPv4 Host addresses, valid
  ports, and the expected one-time nonce format.

Recent commits:

- `76a5b40` — initial installable shell.
- `42d4581` — obsolete shell-cache cleanup.
- `6ea6f13` — visible camera preview and explicit cleanup.
- `371f4c8` — docs: record PWA development status.
- `b4806d7` — production PWA client, QR pairing, and UX hardening.
- `69abf2a` — Phase 1B implementation review.

## Android validation status

Confirmed (in regular Chrome tab on Pixel 9 emulator, Android 17):

- Emulator boot completed.
- Camera HAL repaired to 1 device; Virtual Scene mode configured.
- Service Worker registered under Pages subpath.
- Phase 0: HTTP health, Pairing, Control WS, Frame WS, JPEG display, Reload.
- Phase 0: invalid token → rejected; wrong Origin → rejected; CDP loopback confirmed.

Still to validate manually (physical device or emulator):

1. Install the PWA through Chrome's normal install UI; launch from launcher;
   verify standalone display mode.
2. In standalone mode: LNA prompt, HTTP health, Pairing, Control WS, Frame WS,
   JPEG display, Reload, reconnect after PWA close/reopen.
3. Production viewer: tap, scroll, back/forward, tabs, URL/search, text, logout.
4. Re-run invalid-token and wrong-Origin checks against final Host.
5. Camera / QR — physical-device validation remains pending.

## Known test note

The historical tab regression script can fail when its public YouTube test
video pauses shortly after startup. This was observed independently of the PWA
and CORS changes; it is not yet classified as a PWA regression.

## Decision gates

- Phase 1A is complete when installed standalone PWA operation is confirmed for
  Local Network Access, pairing, control/frame WebSockets, JPEG display, reload,
  reconnect, and retained security. Code implementation is done; physical-device
  validation is the remaining gate.
- Phase 1B implementation exists, but its physical Android/iPhone validation
  remains the next gate.

## Operating rules

- Do not weaken pairing/authentication, Origin validation, CORS restrictions, or
  CDP loopback isolation for PWA work.
- Do not add host discovery, cloud relay, HTTPS on the Rust Host, or a `v0.2.0`
  release in the current phase.
- Keep `main` and released `v0.1.x` artifacts immutable.
