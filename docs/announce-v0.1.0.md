# HWMR v0.1.0 announcement draft

I built Headless Web Music Remote (HWMR) to control a music browser running on my Windows PC from my phone without taking over the PC's mouse or foreground window.

HWMR starts a dedicated Headless Brave instance, streams its rendered view to a phone Web UI, and sends phone controls through CDP. Audio stays on the PC. The first release is a single Windows executable: no Node.js, npm, Rust, or client asset folder is needed at runtime.

What is validated in v0.1.0:

- Headless Brave viewer with tap, scroll, navigation, tabs, and a text bridge.
- PC-side audio, trusted-LAN pairing/authentication, and Windows foreground/cursor isolation.
- Android Emulator viewer, tap, scroll, and ASCII text input.

Feedback wanted:

- Physical Android, especially Japanese Gboard composition and conversion.
- iPhone Safari.
- Real-world reconnect, background, and latency observations.

HWMR is for a trusted LAN only. It has no HTTPS or Internet exposure; do not expose the viewer port to the Internet. CDP remains loopback-only, and Brave Browser is required. Phone audio streaming is not included.

Release: https://github.com/ARTS-Night/Headless-Web-Music-Remote/releases/tag/v0.1.0
