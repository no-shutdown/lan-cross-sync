# LAN Cross-Device Sync Design

## Goal

Build a cross-platform Tauri desktop client for Mac and Windows that makes two or more computers on the same LAN easier to use together.

The tool should support:

- Automatic text and image clipboard sync.
- File and folder transfer through a fixed drop area in the main window.
- First-time pairing, then automatic reconnect on the same LAN.
- Pure LAN direct transfer with no cloud account, server, relay, or third-party sync service.

The first implementation should be useful for personal use while keeping the architecture clean enough to evolve into a packaged product later.

## MVP Scope

The MVP starts with the connection foundation:

- LAN device discovery.
- First-time pairing.
- Remembered trusted devices.
- Automatic reconnect for paired devices.
- Online/offline connection state.
- File target device selection.
- Per-device "receive clipboard" switches.
- System tray/menu bar background operation.
- Start on boot.

After the foundation is stable, the MVP adds:

- Automatic text clipboard broadcast.
- Automatic image clipboard broadcast.
- Receive-side clipboard writes only when that device's switch is enabled.
- Main-window file and folder drop area.
- Receive-side save-location confirmation for incoming files or folders.

The MVP does not include:

- Public internet access.
- Cloud relay or cloud discovery.
- User accounts.
- Clipboard history.
- Per-transfer clipboard confirmation.
- Application whitelist rules for clipboard sync.
- Resumable file transfer.
- End-to-end encryption.

The protocol and local storage format should leave room to add end-to-end encryption later without replacing the whole transport design.

## Recommended Approach

Use a single Tauri application for both Mac and Windows.

Rust owns the background logic:

- Device discovery.
- Pairing.
- Connection management.
- Clipboard monitoring and writes.
- File and folder transfer.
- Local settings.
- Logging.

The Web UI is a control panel:

- Device list.
- Pairing entry points.
- Connection state.
- Per-device clipboard receive switch.
- Default file target selection.
- File and folder drop area.
- Transfer progress.
- Human-readable errors.

The app normally runs in the Mac menu bar or Windows system tray. The main window opens only when the user needs to pair devices, change settings, select a file target, or drop files.

## Internal Modules

The Tauri app should be organized as if it could later become a standalone background service:

- `discovery`: Finds devices on the LAN and publishes this device's presence.
- `pairing`: Handles first-time pairing, pairing codes, trust records, and future key exchange hooks.
- `transport`: Owns connections, message routing, reconnects, and protocol version checks.
- `clipboard`: Watches local clipboard changes, deduplicates remote writes, and writes accepted remote clipboard events.
- `file_transfer`: Scans files and folders, creates transfer tasks, streams file content, and tracks progress.
- `settings`: Persists device name, paired devices, clipboard receive switches, autostart, ports, and defaults.
- `ui_commands`: Exposes safe commands/events between Rust and the Tauri Web UI.
- `logging`: Records operational metadata without storing clipboard text, image bytes, or file content.

Each module should have a narrow interface so it can be tested without the full UI.

## Device Discovery

Each client announces itself on the LAN with minimal metadata:

- Device ID.
- Device name.
- Protocol version.
- App version.
- Listening port.
- Capability flags.

The UI distinguishes:

- Discovered but unpaired devices.
- Paired and online devices.
- Paired but offline devices.

Discovery must not expose clipboard contents, file names, user account data, or local paths.

## Pairing

Pairing is required before clipboard or file transfer is allowed.

Flow:

1. Device A starts pairing and shows a 6-digit one-time code.
2. Device B selects Device A and enters the code.
3. Both devices confirm the pairing session.
4. Each side stores the other device's ID and trust record.
5. Future sessions reconnect automatically on the LAN.

MVP pairing can be simple and LAN-bound. The pairing state and protocol should still include fields that allow future encryption:

- `device_id`
- `session_id`
- `protocol_version`
- `capabilities`
- future public key material

## Connection And Reconnect

The client continuously attempts to connect to paired devices that are discovered on the LAN.

Reconnect should handle:

- App restart.
- Network change.
- Wi-Fi reconnect.
- Sleep and wake.
- Temporary connection loss.

The UI should use simple states:

- Offline.
- Discovering.
- Available to pair.
- Connected.
- Transferring.
- Error.

## Clipboard Sync

Each device watches local clipboard changes for text and images.

When a new local clipboard item is detected, the sender creates a clipboard event with:

- Event ID.
- Source device ID.
- Sequence number.
- Timestamp.
- Content type.
- Content hash.
- Payload metadata.
- Payload bytes or chunk references.

The sender broadcasts the event to paired online devices. The receiver writes the event to its system clipboard only if that device has "receive clipboard" enabled for the sender.

Conflict behavior is simple: the newest clipboard event wins.

To avoid loops:

- A receiver records remote event IDs it writes to the local clipboard.
- When the local clipboard watcher sees that same content immediately after a remote write, it does not rebroadcast it.
- Content hash and source event ID are both used for deduplication.

Text is sent directly. Images are sent with a size limit in MVP. If an image is too large, the UI should tell the user to send it as a file.

## File And Folder Transfer

The main window provides a drop area. The user selects a target device, then drags files or folders into the drop area.

The sender creates a transfer task containing:

- Transfer ID.
- Target device ID.
- Total byte size.
- File count.
- Folder count.
- Relative path list.
- Per-file size and metadata.

Folders are scanned into a directory tree and transferred as structured paths, not as an opaque zip file. The receiver recreates the directory structure at the chosen save location.

The receiver gets an incoming transfer prompt and chooses a save location. If the receiver cancels, the sender sees a clear cancellation state.

The transfer uses streaming chunks so large files are not fully loaded into memory. Control messages and file bytes should be separated enough that a large transfer does not block normal connection messages.

MVP transfer supports:

- Multiple files.
- Folders.
- Preserve relative paths.
- Progress display.
- Cancel.
- Retry the whole failed transfer.
- Open saved folder after completion.

MVP transfer does not support resumable partial transfer.

## Settings

Settings include:

- Local device name.
- Paired devices.
- Per-device receive clipboard switch.
- Default file target device.
- Autostart switch.
- Transfer port strategy.
- Logs export.
- Clear pairing.

The app defaults to background startup on boot, but the user can disable it.

Sensitive future fields, such as encryption keys, should be stored in OS-protected storage or an encrypted local store. The MVP storage format should already reserve a place for those fields.

## Permissions And Platform Notes

The first-run experience should guide the user through required permissions.

Mac:

- Local network permission.
- Clipboard access behavior.
- Notification permission.
- Menu bar operation.
- File save dialog behavior.

Windows:

- Firewall allow prompt for local network.
- System tray operation.
- Autostart setup.
- Clipboard access behavior.
- File save dialog behavior.

The app should explain permission problems in plain language and point the user to the relevant setting when possible.

## Error Handling

Errors should be actionable and non-technical in the UI.

Important error classes:

- Device discovery failed.
- Pairing code expired or invalid.
- Device disconnected.
- Clipboard read failed.
- Clipboard write failed.
- Image too large for clipboard sync.
- File or folder scan failed.
- Receiver rejected the transfer.
- Save location unavailable.
- Transfer interrupted.

Example UI messages:

- "Target device is offline."
- "The receiver cancelled the save prompt."
- "This image is too large for clipboard sync. Send it as a file instead."
- "The connection was interrupted. Retry the transfer."

## Logging

Logs should help debugging without capturing private content.

Allowed log data:

- Connection state changes.
- Device IDs and friendly names.
- Pairing state transitions.
- Transfer IDs, sizes, counts, and status.
- Clipboard event type, hash, and size.
- Error codes.

Forbidden log data:

- Clipboard text.
- Image bytes.
- File content.
- Full local file paths beyond what is needed for transfer status.
- Secrets or future encryption keys.

## Testing Strategy

Rust core modules should be tested first:

- Protocol message parsing and version handling.
- Pairing state machine.
- Trust record persistence.
- Clipboard deduplication and loop prevention.
- File and folder tree scanning.
- Transfer task state machine.
- Chunking and reassembly.

Integration tests should run two local client instances where possible:

- Discover another instance.
- Pair with a code.
- Reconnect after restart.
- Send text clipboard event.
- Send image clipboard event within size limit.
- Transfer files.
- Transfer folders.

Manual Mac/Windows acceptance tests:

- First launch permissions.
- Device discovery on same Wi-Fi.
- First-time pairing.
- Automatic reconnect.
- Mac copy, Windows paste.
- Windows copy, Mac paste.
- Screenshot/image copy and paste.
- File transfer with save-location prompt.
- Folder transfer with directory structure preserved.
- Sleep/wake reconnect.
- Autostart after reboot.

## Future Enhancements

Likely follow-up features:

- End-to-end encryption during pairing and transport.
- Clipboard history and restore.
- Floating file drop target.
- System right-click/share menu integration.
- Resumable file transfer.
- Per-device bandwidth limits.
- Per-file conflict handling.
- Optional receive directory defaults.
- Better multi-device routing rules.

## Approved Implementation Decisions

This section records the decisions approved for the unattended implementation pass. The
implementation covers iterations 2 through 7 and Simplified Chinese support. Iteration 8
features remain explicitly out of scope: end-to-end encryption, resumable transfers,
bandwidth limits, and clipboard history.

### Pairing and protocol

- Keep UDP discovery as the device-location mechanism. The runtime registry stores the
  source socket address received with each discovery packet; addresses are not persisted
  because DHCP can change them.
- Increment the protocol version for the new business messages. A peer using the old
  foundation protocol may still be visible as unsupported, but it cannot pair or transfer
  data.
- Pairing requests are directed to the selected device and contain a request ID, target
  device ID, sender device information, and the six-digit code. Responses contain a stable
  reason code rather than localized text.
- The accepting device returns an accepted response and keeps the request pending until it
  receives a confirmation. Both sides persist the peer only after the confirmation path;
  retries use the same request ID and are idempotent.
- A successful code is single-use. Invalid, expired, cancelled, self-targeted, unsupported,
  and duplicate requests have explicit test-covered outcomes.
- Pairing is intentionally LAN-trust based in this release. The code is not a substitute
  for cryptographic authentication and must not be described as protection from a hostile
  local network.

### Transport

- Add a Rust-owned Tokio TCP listener using the existing device service port. UDP and TCP
  may bind the same numeric port.
- Use a length-delimited frame with a bounded control payload. Control messages are JSON
  envelopes with message kind and request ID; file data is streamed as bounded binary
  chunks and is never assembled as one in-memory payload.
- A connection starts with a device hello, rejects unpaired business messages, sends
  periodic heartbeats, applies read/write timeouts, and updates the peer registry on
  connect, disconnect, and reconnect.
- One connection manager owns each paired peer. Clipboard messages have priority over file
  chunks so a large transfer cannot make the control channel unresponsive.
- The transport emits stable event names and error codes to the Tauri layer. It never logs
  clipboard text, image bytes, file content, or secrets.

### Clipboard

- Rust watches the local system clipboard through a cross-platform clipboard implementation
  and polls at a bounded interval suitable for desktop use.
- Text and images share one event model containing event ID, source device ID, timestamp,
  content type, content hash, metadata, and payload.
- The sender sends only to paired, connected peers whose receive-clipboard setting is on.
  The receiver records remote event IDs and hashes before writing to the local clipboard,
  preventing rebroadcast loops.
- Images are encoded into a portable format before transport and have a fixed compressed
  size limit for MVP. Oversized images are rejected with a localized instruction to send
  them as files.
- Clipboard failures are reported as stable error keys and are not fatal to the connection.

### File and folder transfer

- The React drop zone receives only dropped paths. Rust scans the paths, rejects unreadable
  entries, creates a relative-path manifest, and sends metadata before file bytes.
- The receiver emits an incoming-transfer event. The UI opens a directory picker, then
  sends accept or reject back to Rust.
- Files are streamed in bounded chunks into temporary `.part` files. Relative paths are
  normalized and traversal outside the selected destination is rejected. Completed files
  are atomically renamed into place.
- The MVP supports files, nested folders, progress, cancellation, and whole-transfer retry.
  It does not overwrite silently and does not resume partial transfers.
- Transfer progress is identified by transfer ID and includes counts and byte totals, never
  file contents.

### Simplified Chinese support

- Add a typed frontend translation dictionary with `zh-CN` and `en-US` entries. Simplified
  Chinese is the default locale; English remains available as a fallback and test locale.
- All visible React text, validation messages, pairing errors, transfer states, empty states,
  tray labels, and setup guidance use translation keys. Rust returns error codes, not UI
  language strings.
- The selected locale is persisted locally and can be changed without changing protocol or
  device data. Missing translations fall back to English and then to the key only as a
  developer-visible last resort.

### Productization and acceptance

- Keep tray residency, close-to-tray, and autostart behavior while adding clear first-run
  guidance for Windows firewall and macOS local-network/clipboard permissions.
- Replace the development-only CSP with a production-safe Tauri configuration and keep
  capabilities limited to the commands and dialog operations actually used.
- The Windows release job produces NSIS and MSI artifacts. A macOS build job produces the
  corresponding app/DMG artifacts on macOS. Unsigned local artifacts are acceptable for
  development acceptance; signing and notarization remain release operations.
- Acceptance must cover two real devices for discovery, pairing, reconnect, text, image,
  files, folders, sleep/wake, autostart, and uninstall/reinstall settings behavior. A
  single Windows machine can cover unit and loopback tests but cannot replace the two-device
  acceptance pass.

## Open Decisions

No open MVP decisions remain from the brainstorming session.

The approved MVP direction is: Tauri single client, pure LAN, first-time pairing, automatic reconnect, automatic text and image clipboard sync, per-device receive switches, file and folder drop area in the main window, receiver confirms save location, and implementation starts from the discovery/pairing/connection foundation.
