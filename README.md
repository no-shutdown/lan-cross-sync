# LAN Cross Sync

LAN Cross Sync is a Tauri desktop app foundation for syncing clipboard content and files between a Mac and a Windows PC on the same LAN.

## Current Scope

Implemented:

- Local device identity persisted to `settings.json`.
- Versioned LAN discovery packet encoding, decoding, and periodic broadcast.
- Pairing state with 6-digit temporary codes.
- Runtime peer registry for discovered and paired devices.
- Tauri command layer for dashboard state and pairing controls.
- React control panel with pairing, paired devices, discovered devices, startup, and future file drop area.
- Desktop autostart switch backed by the OS autostart plugin.
- Tray icon with show and quit actions; closing the window hides it to the tray.

Deferred:

- Discovery receive loop and live population from other machines.
- Full two-device pairing handshake.
- Clipboard text/image sync.
- File and folder transfer.
- Packaging and cross-machine acceptance tests.

## Development

Install dependencies:

```powershell
pnpm install
```

Run the app in development:

```powershell
pnpm tauri dev
```

On this Windows machine, Rustup installed Cargo at `C:\Users\13565\.cargo\bin`. If `pnpm tauri dev` cannot find `cargo`, prefix the current shell PATH:

```powershell
$env:PATH = 'C:\Users\13565\.cargo\bin;' + $env:PATH
pnpm tauri dev
```

## Verification

```powershell
C:\Users\13565\.cargo\bin\cargo.exe test --manifest-path src-tauri\Cargo.toml
pnpm build
```

The app writes local settings under the Tauri app config directory. On Windows that resolved to:

```text
C:\Users\13565\AppData\Roaming\com.local.lancrosssync\settings.json
```
