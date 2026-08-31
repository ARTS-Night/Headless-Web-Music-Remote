# Headless Web Music Remote

HWMR runs Headless Brave on Windows and forwards rendered web-music pixels to a phone over a trusted LAN. Phone controls travel through the local host to Chrome DevTools Protocol; Windows foreground and mouse input remain untouched, and audio stays on the PC.

## Start and pair

Run `cargo run --bin hwmr-host`, then open the printed viewer URL. The host prints a one-time pairing code; enter it in the client. A successful pairing stores a random in-memory session token in browser `localStorage`, so a page reload does not require pairing again. Restarting the host invalidates that token.

## Security scope

CDP is loopback-only and is never exposed to LAN clients. Browser data, diagnostics, controls, and frames require the session token after pairing. There is no HTTPS yet: use HWMR only on a trusted LAN and never expose it directly to the Internet.
