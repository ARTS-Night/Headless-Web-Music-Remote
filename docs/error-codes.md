# HWMR error codes

The web clients show a short human-readable message followed by one stable
code. Codes are part of the user-facing interface and keep their meaning once
published.

## Network and browser

### HWMR-NET-001 — Host unreachable

The browser could not reach the configured LAN Host. Confirm that HWMR is
running and that the phone and PC are on the same trusted network.

### HWMR-BROWSER-002 — Direct LAN connection unsupported

The browser could not connect from the HTTPS Pages client. Use the offered
**Open HWMR** local-controller link; it keeps the connection on the LAN.

## Pairing and QR

### HWMR-AUTH-001 — Invalid pairing code

The manual pairing code was rejected or has expired. Enter the current code
shown by HWMR.

### HWMR-QR-001 — Invalid QR code

The scanned data could not be decoded as a valid HWMR bootstrap payload.

### HWMR-QR-002 — Not an HWMR connection link

The QR link is not for the official HWMR Pages client.

### HWMR-QR-003 — Unsupported QR version

The QR payload uses a protocol version this client does not understand.

### HWMR-QR-004 — QR pairing nonce expired or used

The short-lived one-time nonce has expired or was already consumed. Generate a
new QR on the PC.

## Camera and session

### HWMR-CAM-001 — Camera permission denied

Camera access was denied. Allow camera access for the HWMR Pages site or use
manual connection.

### HWMR-CAM-002 — Camera unavailable

No usable camera was available to the browser.

### HWMR-WS-004 — Connection lost

The authenticated control or frame connection closed. Reconnect or pair
again.

## Security notes

QR payloads contain only a version, private-LAN Host, port, and short-lived
nonce. A final session token, password, cookie, or CDP credential is never
placed in a QR code or URL.
