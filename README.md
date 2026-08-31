# Headless Web Music Remote

HWMR runs Headless Brave on Windows and forwards rendered web-music pixels to a phone over a trusted LAN. Phone controls travel through the local host to Chrome DevTools Protocol; Windows foreground and mouse input remain untouched, and audio stays on the PC.

## MVP status

Bootstrap complete. The first milestone is Headless Brave → CDP screencast → binary JPEG WebSocket → browser viewer.

## Security scope

CDP is loopback-only. The LAN client will require pairing; until HTTPS is added, it is for trusted networks only.
