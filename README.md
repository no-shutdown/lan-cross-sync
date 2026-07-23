# LAN Cross Sync

LAN Cross Sync is a Tauri desktop app foundation for syncing clipboard content and files between a Mac and a Windows PC on the same LAN.

## Current Scope

Implemented:

- Versioned LAN discovery and peer endpoint tracking.
- Six-digit pairing handshake with paired-peer authorization.
- Framed TCP transport with reconnect and heartbeat handling.
- Text and image clipboard synchronization with loop prevention and size limits.
- File and folder transfer with safe paths, progress events, cancellation, and atomic finalization.
- Simplified Chinese and English UI switching.
- Desktop autostart, tray behavior, drag-and-drop, and native file dialogs.
- Windows NSIS and MSI packaging.

Deferred by the approved MVP scope:

- End-to-end encryption, resumable transfers, bandwidth limits, and clipboard history.

## Handoff

The recommended implementation order and acceptance criteria are documented in
[`docs/DEVELOPMENT_ROADMAP.md`](docs/DEVELOPMENT_ROADMAP.md). Start from the
latest commit and keep each iteration independently buildable and testable.

## Development

Install dependencies:

```powershell
pnpm install
```

Run the app in development:

```powershell
pnpm tauri dev
```

If `pnpm tauri dev` cannot find `cargo`, prefix the current shell PATH:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
pnpm tauri dev
```

## Verification

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
pnpm build
```

Package the Windows application:

```powershell
pnpm tauri build
```

The generated installers are under `src-tauri\target\release\bundle\nsis\` and
`src-tauri\target\release\bundle\msi\`. See
[`docs/BUILD_AND_TEST.md`](docs/BUILD_AND_TEST.md) for two-device acceptance steps
and troubleshooting.

The app writes local settings under the Tauri app config directory. On Windows this is typically:

```text
%APPDATA%\com.local.lancrosssync\settings.json
```
