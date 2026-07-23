# LAN Cross Sync MVP Sync And Localization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the LAN Cross Sync MVP from real pairing through reconnecting transport, text/image clipboard sync, file/folder transfer, productization checks, and Simplified Chinese UI support.

**Architecture:** Keep UDP discovery as the location layer and add a Rust-owned Tokio TCP service on the existing device port. Pairing is a directed UDP request/response/confirmation flow; only confirmed peers enter persistent settings and the transport allow-list. Rust owns clipboard and filesystem work, while React renders translated state and invokes narrow Tauri commands.

**Tech Stack:** Tauri v2, Rust 2021, Tokio, Serde/JSON, Tokio TCP, `arboard`, `image`, `sha2`, `walkdir`, Tauri dialog plugin, React 19, TypeScript, Vite, Vitest, pnpm.

---

## Scope and Safety Rules

This plan implements iterations 2 through 7 from `docs/DEVELOPMENT_ROADMAP.md` and the approved `zh-CN`/`en-US` UI dictionary. It does not implement end-to-end encryption, resumable transfer, bandwidth limits, or clipboard history.

The existing worktree contains a user-owned `src-tauri/Cargo.toml` status change and generated package-manager state. Never stage or revert those unrelated changes. Every commit below stages only the files named by its task.

The Rust protocol will move to version 2. Old version-1 peers may remain visible as unsupported, but they must not be allowed to pair or transfer data. Persisted settings receive `serde(default)` values for new fields so existing JSON remains readable.

## File Map

Create:

- `src-tauri/src/transport.rs`: TCP listener, reconnect manager, peer sessions, bounded frames.
- `src-tauri/src/clipboard.rs`: clipboard event model, deduplication, polling and writes.
- `src-tauri/src/file_transfer.rs`: manifest scanning, path validation, transfer state and streaming helpers.
- `src-tauri/src/ui_events.rs`: event names and serializable UI event payloads.
- `src/i18n/index.ts`: locale type, dictionaries, fallback lookup and locale persistence.
- `src/i18n/index.test.ts`: translation and fallback tests.
- `docs/ACCEPTANCE.md`: two-device acceptance matrix and platform prerequisites.

Modify:

- `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`: clipboard, image, hash, walk, dialog dependencies.
- `src-tauri/src/domain.rs`: runtime endpoint, protocol version, transfer and locale settings types.
- `src-tauri/src/protocol.rs`: version-2 pairing and transport message envelopes.
- `src-tauri/src/pairing.rs`: one-time pairing sessions, pending confirmations and stable rejection codes.
- `src-tauri/src/discovery.rs`: source endpoint capture and pairing-message dispatch.
- `src-tauri/src/registry.rs`: endpoint map and connection state transitions.
- `src-tauri/src/settings.rs`: new-field defaults and atomic peer persistence.
- `src-tauri/src/commands.rs`: pairing request, transfer commands, locale command and UI-safe errors.
- `src-tauri/src/lib.rs`: service startup, managed state, event wiring and shutdown.
- `src-tauri/src/error.rs`: stable error codes and localized-safe serialization.
- `src-tauri/capabilities/default.json`: dialog permission.
- `src-tauri/tauri.conf.json`: production CSP and bundle metadata.
- `src/App.tsx`, `src/App.css`, `src/lib/api.ts`, `src/lib/types.ts`: pairing form, transfer UI, translated controls and progress.
- `package.json`, `pnpm-lock.yaml`: test script and Vitest dependency.
- `README.md`, `docs/DEVELOPMENT_ROADMAP.md`: current commands, feature status and acceptance links.

## Task 1: Versioned Domain, Persistence, And Endpoint Registry

**Files:**

- Test/modify: `src-tauri/src/domain.rs`, `src-tauri/src/settings.rs`, `src-tauri/src/registry.rs`
- Modify: `src-tauri/src/protocol.rs`, `src-tauri/src/discovery.rs`

- [ ] **Step 1: Write failing tests for version 2 and endpoint behavior**

Add tests that assert:

```rust
assert_eq!(crate::protocol::PROTOCOL_VERSION, 2);
let endpoint = "192.0.2.10:45731".parse().unwrap();
registry.mark_discovered_at(remote.clone(), endpoint);
assert_eq!(registry.endpoint(&remote.id), Some(endpoint));
```

Add a settings fixture without new fields and assert it loads with `zh-CN`, empty transfer state, and no encryption material. Add a removal test that removes both the peer and its endpoint.

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml domain:: settings:: registry:: -- --nocapture
```

Expected: compilation or assertion failures identify the missing version, defaults, endpoint API, and cleanup behavior.

- [ ] **Step 3: Implement the minimal shared types**

Add `RuntimeEndpoint { address: SocketAddr, last_seen: Instant }` to the in-memory registry only. Keep `DeviceInfo.port` as the shared UDP/TCP service port. Add `#[serde(default)]` to new persisted fields and default the UI locale to `zh-CN`. Change the protocol constant and update discovery/device constructors to version 2.

- [ ] **Step 4: Run the focused tests and then the full Rust suite**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all existing and new tests pass.

- [ ] **Step 5: Commit the domain slice**

```powershell
git add src-tauri/src/domain.rs src-tauri/src/settings.rs src-tauri/src/registry.rs src-tauri/src/protocol.rs src-tauri/src/discovery.rs
git commit -m "feat: add versioned peer endpoints"
```

## Task 2: Complete Pairing Over Directed UDP

**Files:**

- Test/modify: `src-tauri/src/protocol.rs`, `src-tauri/src/pairing.rs`, `src-tauri/src/discovery.rs`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/error.rs`

- [ ] **Step 1: Write failing protocol and state-machine tests**

Add golden JSON tests for `PairingRequest`, `PairingResponse`, and `PairingConfirm` with `request_id`, `target_device_id`, `session_id`, `from_device`, `accepted`, and `reason_code`. Add tests for valid code, wrong code, expired code, target mismatch, one-time success, duplicate request ID, and confirmation retry.

- [ ] **Step 2: Run pairing tests and verify RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml protocol::tests pairing::tests discovery::tests -- --nocapture
```

Expected: the new message constructors and state transitions are absent or fail their assertions.

- [ ] **Step 3: Implement pairing messages and pending confirmations**

Use stable rejection codes: `invalid_code`, `expired_code`, `target_mismatch`, `unsupported_protocol`, `self_device`, `already_paired`, and `request_timeout`. Keep a pending request for the pairing-code TTL, cache accepted responses by request ID, persist a peer on confirmation, and make repeated requests idempotent.

- [ ] **Step 4: Route directed UDP packets using runtime endpoints**

Extend the receive loop to retain the sender address, route only messages addressed to the local device, reject self and unsupported messages, and send responses to the request source. Do not persist socket addresses.

- [ ] **Step 5: Add Tauri commands for request and confirmation**

Expose `request_pairing(device_id, code)`, `cancel_pairing`, and dashboard pairing status. Return error codes in the serialized error object; do not return English UI text from Rust.

- [ ] **Step 6: Run tests and commit**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
git add src-tauri/src/protocol.rs src-tauri/src/pairing.rs src-tauri/src/discovery.rs src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/error.rs
git commit -m "feat: complete LAN pairing handshake"
```

## Task 3: TCP Framing, Handshake, Heartbeat, And Reconnect

**Files:**

- Create/modify: `src-tauri/src/transport.rs`, `src-tauri/src/protocol.rs`, `src-tauri/src/registry.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`

- [ ] **Step 1: Write failing frame and connection-state tests**

Test a length-delimited frame round trip, truncated header rejection, payload limit rejection, hello rejection for unpaired IDs, heartbeat timeout, and reconnect backoff capped at five seconds.

- [ ] **Step 2: Run transport tests and verify RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml transport::tests -- --nocapture
```

Expected: the new transport module or frame helpers are missing.

- [ ] **Step 3: Implement bounded framing**

Use a four-byte big-endian header length, a JSON envelope containing protocol version, frame kind, request ID, and payload length, and a bounded binary payload. Reject headers over 1 MiB and file chunks over 64 KiB. Use `tokio::io::AsyncReadExt` and `AsyncWriteExt`; never call `read_to_end` on a peer stream.

- [ ] **Step 4: Implement the listener and per-peer connection manager**

Bind TCP on the local service port. On accept, require a hello containing a known paired device ID. For discovered paired endpoints, connect with exponential backoff, send hello, run a heartbeat every 5 seconds, close after three missed heartbeats, and update the registry state. Use separate reader and writer tasks with bounded channels so control frames are not starved by file chunks.

- [ ] **Step 5: Verify with loopback integration tests**

Start two in-process listeners on ephemeral ports, register each other as paired, exchange hello and heartbeat, stop one listener, assert `offline`, restart it, and assert `connected`.

- [ ] **Step 6: Run tests and commit**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
git add src-tauri/src/transport.rs src-tauri/src/protocol.rs src-tauri/src/registry.rs src-tauri/src/lib.rs src-tauri/src/commands.rs
git commit -m "feat: add paired peer transport"
```

## Task 4: Clipboard Event Model And Text/Image Sync

**Files:**

- Create/modify: `src-tauri/src/clipboard.rs`, `src-tauri/src/protocol.rs`, `src-tauri/src/ui_events.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/transport.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`

- [ ] **Step 1: Add dependencies and write pure event tests**

Add `arboard`, `image` with PNG support, and `sha2`. Write tests for text event hashing, image metadata, remote event ID deduplication, same-content deduplication, newest-event ordering, disabled peer filtering, and the 10 MiB compressed-image limit.

- [ ] **Step 2: Run clipboard tests and verify RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml clipboard::tests -- --nocapture
```

Expected: the event model and deduplication helpers are missing.

- [ ] **Step 3: Implement clipboard event processing**

Create `ClipboardEvent` with event ID, source ID, timestamp, content type, SHA-256 hash, metadata, and payload. Poll the local clipboard on a dedicated blocking thread, encode images as PNG, and send only to paired peers with `receive_clipboard = true`. Write received data through the same dedicated clipboard worker and record it before writing to prevent loops.

- [ ] **Step 4: Wire clipboard frames and UI events**

Route text/image clipboard frames through the transport manager. Emit `clipboard-status` with stable states such as `sent`, `received`, `disabled`, `too_large`, and `error`, without payload content.

- [ ] **Step 5: Run the full Rust suite and commit**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/clipboard.rs src-tauri/src/protocol.rs src-tauri/src/transport.rs src-tauri/src/ui_events.rs src-tauri/src/lib.rs src-tauri/src/commands.rs
git commit -m "feat: sync text and image clipboard"
```

## Task 5: File And Folder Manifest, Streaming, And Cancellation

**Files:**

- Create/modify: `src-tauri/src/file_transfer.rs`, `src-tauri/src/protocol.rs`, `src-tauri/src/ui_events.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/transport.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Add `walkdir` and write failing manifest/path tests**

Test single files, nested folders, relative path preservation, unreadable entries, `..` traversal rejection, duplicate names, and total byte/count calculation using temporary directories.

- [ ] **Step 2: Run file-transfer tests and verify RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml file_transfer::tests -- --nocapture
```

Expected: manifest and path-safety helpers are missing.

- [ ] **Step 3: Implement manifest scanning and safe destination paths**

Represent each entry with relative path, directory/file kind, byte size, and modified time. Normalize separators, reject absolute paths and traversal, and create destination directories only beneath the user-selected root.

- [ ] **Step 4: Implement offer, accept/reject, chunk, cancel, and complete messages**

Keep the manifest in a bounded control message. Stream each file in 64 KiB chunks into a `.part` file, flush and rename only after the expected byte count and SHA-256 hash match. Cancel closes and removes partial files. A retry creates a new transfer ID and starts from the beginning.

- [ ] **Step 5: Add loopback transfer tests**

Transfer one file, multiple files, and a nested folder between two in-process managers. Assert byte equality, directory structure, progress totals, cancellation cleanup, and rejection of unsafe paths.

- [ ] **Step 6: Run tests and commit**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/file_transfer.rs src-tauri/src/protocol.rs src-tauri/src/transport.rs src-tauri/src/ui_events.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: transfer files and folders over LAN"
```

## Task 6: Tauri Dialogs, Commands, Events, And Service Lifecycle

**Files:**

- Modify: `src-tauri/Cargo.toml`, `src-tauri/capabilities/default.json`, `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/ui_events.rs`
- Modify: `src/lib/api.ts`, `src/lib/types.ts`

- [ ] **Step 1: Add the Tauri dialog plugin and capability**

Add `tauri-plugin-dialog` and grant only its directory-open permission. Keep filesystem reads/writes in Rust, scoped to the selected destination and dropped source paths.

- [ ] **Step 2: Expose typed commands and event payloads**

Add wrappers for `request_pairing`, `send_paths`, `accept_transfer`, `reject_transfer`, `cancel_transfer`, `retry_transfer`, and `set_locale`. Define TypeScript discriminated unions matching Rust event payloads.

- [ ] **Step 3: Start and stop background services with the app lifecycle**

Create one `AppServices` owner in `lib.rs` for discovery, transport, clipboard, and transfers. Start it after settings load, stop it on application exit, and keep tray close behavior unchanged. Service errors update peer state and emit UI events instead of panicking.

- [ ] **Step 4: Run Rust and TypeScript checks**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
pnpm build
```

Expected: all Rust tests pass and TypeScript/Vite build succeeds.

## Task 7: Simplified Chinese UI And Transfer UX

**Files:**

- Create: `src/i18n/index.ts`, `src/i18n/index.test.ts`
- Modify: `package.json`, `pnpm-lock.yaml`, `src/App.tsx`, `src/App.css`, `src/lib/api.ts`, `src/lib/types.ts`

- [ ] **Step 1: Add Vitest and write failing dictionary tests**

Add `vitest` and a `test` script. Test that `zh-CN` resolves Chinese strings, `en-US` resolves English strings, missing locale keys fall back to English, and locale persistence uses the expected storage key.

- [ ] **Step 2: Run the frontend tests and verify RED**

```powershell
pnpm test -- --run src/i18n/index.test.ts
```

Expected: the dictionary module and test script are missing.

- [ ] **Step 3: Implement the translation dictionary and locale hook**

Define typed keys for every visible label, button, state, error, pairing message, transfer message, startup setting, empty state, and setup hint. Default to `zh-CN`, allow switching to `en-US`, persist the selected locale, and fall back from missing locale keys to English.

- [ ] **Step 4: Replace hard-coded UI text and add pairing/transfer controls**

Add a six-digit numeric input and pair button to discovered-device cards. Add selected target state, drop-zone drag/drop handling, incoming-transfer prompt, progress rows, cancel/retry/open-folder actions, connection status labels, and a locale selector. Keep dimensions stable and make long Chinese text wrap without overlapping controls.

- [ ] **Step 5: Run frontend tests and production build**

```powershell
pnpm test -- --run
pnpm build
```

Expected: all translation tests pass and Vite produces `dist` successfully.

- [ ] **Step 6: Commit the UI slice**

```powershell
git add package.json pnpm-lock.yaml src/App.tsx src/App.css src/lib/api.ts src/lib/types.ts src/i18n
git commit -m "feat: add Chinese UI and transfer controls"
```

## Task 8: Productization, Packaging, And Acceptance Documentation

**Files:**

- Modify: `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/lib.rs`, `README.md`, `docs/DEVELOPMENT_ROADMAP.md`
- Create: `docs/ACCEPTANCE.md`

- [ ] **Step 1: Configure production-safe security and bundles**

Set a non-null production CSP compatible with the Tauri IPC scheme, keep only required capabilities, configure Windows NSIS/MSI targets and macOS DMG/app targets, and preserve the existing icons and current-user installation behavior.

- [ ] **Step 2: Add first-run and permission guidance**

Expose a localized setup panel for Windows firewall/private-network approval and macOS local-network/clipboard permissions. Keep guidance actionable and avoid claiming encryption or hostile-network protection.

- [ ] **Step 3: Document acceptance and package commands**

Write a matrix for discovery, pairing, reconnect, text, image, file, folder, sleep/wake, tray, autostart, install, uninstall, and settings retention. Include Windows commands:

```powershell
pnpm install --frozen-lockfile
cargo test --manifest-path src-tauri\Cargo.toml
pnpm build
pnpm tauri build --bundles nsis --no-sign
pnpm tauri build --bundles msi --no-sign
```

Document that macOS artifacts require a macOS host or CI runner and that two-device acceptance cannot be replaced by loopback tests.

- [ ] **Step 4: Run final verification**

```powershell
cargo fmt --all -- --check
cargo test --manifest-path src-tauri\Cargo.toml
pnpm test -- --run
pnpm build
pnpm tauri build --no-sign
git diff --check
git status --short
```

Expected: formatting, Rust tests, frontend tests, frontend build, and Windows package creation succeed. Any remaining platform limitation or warning must be reported explicitly.

- [ ] **Step 5: Commit documentation and release configuration**

```powershell
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json src-tauri/src/lib.rs README.md docs/DEVELOPMENT_ROADMAP.md docs/ACCEPTANCE.md
git commit -m "docs: add MVP acceptance and packaging guidance"
```

## Plan Self-Review

- Scope coverage: Tasks 1-2 cover pairing and trusted persistence; Task 3 covers transport, heartbeat, reconnect, and state; Tasks 4-5 cover text/image clipboard and file/folder transfer; Tasks 6-7 cover Tauri lifecycle, dialogs, UI, and Simplified Chinese; Task 8 covers CSP, capabilities, packaging, permissions, and two-device acceptance.
- Placeholder scan: no task relies on `TBD`, `TODO`, or unspecified future work; out-of-scope iteration 8 items are explicitly named.
- Type consistency: pairing IDs use `request_id` and `session_id`; transfer operations use `transfer_id`; UI commands and event payloads use the same snake_case DTO names as Rust serialization.
- Residual acceptance gap: a Windows development environment cannot prove macOS compilation or real Mac/Windows behavior. The plan requires a macOS host or CI runner for that final portion and reports it rather than treating Windows loopback as equivalent.
