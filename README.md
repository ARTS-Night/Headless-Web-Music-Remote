# Headless Web Music Remote (HWMR)

HWMR runs Brave headlessly on a Windows PC, keeps audio on that PC, and sends rendered JPEG frames plus controls to a phone on a trusted LAN. The phone never controls the Windows foreground window or mouse: every remote action goes through CDP to the dedicated headless Brave instance.

**At a glance:** Headless Brave runs on the Windows PC; the phone receives its rendered browser view and controls it through HWMR/CDP. Audio remains on the PC, while the Windows foreground window and cursor remain untouched.

## Requirements

- Windows 10 or later
- [Brave](https://brave.com/download/) installed for the current machine
- A phone and PC on the same trusted LAN

Node.js, npm, Vite, Rust, and a browser dev server are **not** needed to run the release executable.

## Install and run

1. Download `hwmr.exe` from the release package.
2. Run `hwmr.exe`.
3. On the phone, open the printed `Phone (trusted LAN)` URL.
4. Enter the one-time pairing code printed by HWMR.

The host prints its dedicated profile location, local URL, LAN URL when a private LAN address is detected, pairing code, and CDP binding. Pairing tokens exist only in host memory; restarting HWMR invalidates existing phone sessions.

## Feedback and device validation

- Found a reproducible problem? [Open a Bug Report](https://github.com/ARTS-Night/Headless-Web-Music-Remote/issues/new?template=bug_report.yml).
- Tested a phone or keyboard? [Share Device Validation](https://github.com/ARTS-Night/Headless-Web-Music-Remote/issues/new?template=device_validation.yml), including successful results.
- Have an idea? [Open a Feature Request](https://github.com/ARTS-Night/Headless-Web-Music-Remote/issues/new?template=feature_request.yml).

See [the demo capture guide](docs/recording-guide.md) for the planned README GIF. No visual link is included until an asset is available.

## Controls

- Tap and swipe the rendered screen to control Brave.
- Use the bottom bar for Back, tabs, play/pause, Reload, and Forward.
- Use **URL or search** to navigate.
- Tapping a remote text field opens the phone keyboard. ASCII, space, Backspace, and Enter are validated on the Android Emulator. Japanese Gboard on a physical device is still pending validation.

## Audio behavior

Audio always stays on the Windows PC speakers or headphones. The phone receives visuals and sends controls only.

## Security model

- Viewer HTTP/WebSocket listens on the PC network interfaces so a trusted-LAN phone can connect.
- Pairing is mandatory; protected HTTP, control WebSocket, and frame WebSocket reject unauthenticated clients.
- CDP is fixed to `127.0.0.1:9229` and is never exposed to LAN clients.
- HWMR does not configure UPnP, port forwarding, or Internet exposure. Do not expose its viewer port to the Internet.
- HWMR uses its own profile under `%LOCALAPPDATA%\HWMR\browser-profile-v7`; it never uses the normal Brave profile.

## Configuration

Defaults are deliberately small and stable:

| Setting | Default | Override |
| --- | --- | --- |
| Viewer port | `8787` | `HWMR_HOST_PORT` |
| CDP port | `9229` loopback-only | fixed |
| JPEG quality | `60` | `HWMR_JPEG_QUALITY` (1–100) |
| Frame cadence | every second frame | `HWMR_EVERY_NTH_FRAME` (>0) |
| Viewport | `430x932` | fixed |
| Brave executable | standard Windows locations | `HWMR_BRAVE_PATH` |
| Dedicated profile | `%LOCALAPPDATA%\HWMR\browser-profile-v7` | `HWMR_PROFILE_DIR` |

If Brave cannot be found, install it in the standard location or set `HWMR_BRAVE_PATH` to `brave.exe`. If startup reports an existing `DevToolsActivePort`, close the prior HWMR instance that owns its dedicated profile before retrying.

## Development and build

```powershell
cargo test
cargo build --release
```

The release artifact is `target\release\hwmr.exe`. The web client is compiled into the executable, so no sidecar asset directory is required.

Runtime scenarios require a running HWMR host and its pairing token:

```powershell
node scripts/audio-scenario.mjs
node scripts/tab-scenario.mjs
node scripts/text-input-scenario.mjs
```

## Known limitations

- This is for trusted LAN use, not Internet remote access.
- Physical Android Japanese Gboard validation remains pending.
- iPhone validation remains pending.
- The transport is JPEG/WebSocket (`quality=60`, every second frame), not WebRTC or video streaming.

## Post-release policy

HWMR v0.1.x is in feedback and validation mode. Until results from roughly 3–5 independent environments are collected, new features are generally deferred. Reproducible real-world bugs should receive the smallest practical patch fix; feature requests are collected in Issues. See [CONTRIBUTING.md](CONTRIBUTING.md).
