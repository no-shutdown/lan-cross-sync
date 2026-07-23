# Discovery Receive Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a running LAN Cross Sync app listen for LAN discovery broadcasts and show other devices in the existing Discovered devices UI.

**Architecture:** Keep discovery logic in Rust. The discovery module owns packet decoding and receive-loop behavior; `lib.rs` only wires background tasks at app startup. The UI already polls `get_dashboard_state`, so once `PeerRegistry` is updated by the receive loop no frontend changes are required.

**Tech Stack:** Tauri v2, Rust, Tokio UDP sockets, Serde JSON, React/Vite existing UI.

---

## Scope Check

This plan implements only follow-up stage 1 from the foundation plan:

- Bind a UDP listener on the local discovery port.
- Decode incoming discovery packets.
- Ignore this app's own discovery packets.
- Ignore unsupported protocol versions and non-discovery messages.
- Update `PeerRegistry` so discovered unpaired devices appear in the UI.
- Mark paired offline/error devices as discovered when they are seen again.

This plan deliberately does not implement:

- Pairing handshake between two devices.
- TCP transport or reconnect attempts.
- Clipboard sync.
- File/folder transfer.
- Firewall helper UI.

## File Structure

- `src-tauri/src/commands.rs`: change shared app state fields to `Arc<Mutex<_>>` so background tasks and Tauri commands use the same registry.
- `src-tauri/src/discovery.rs`: add pure packet-application helper and UDP receive loop.
- `src-tauri/src/lib.rs`: create shared state handles and spawn the receive loop next to the existing announce loop.
- `docs/superpowers/plans/2026-07-23-discovery-receive-loop.md`: track execution status.

## Task 1: Share Runtime State With Background Tasks

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [x] **Step 1: Update `AppState` fields to use `Arc<Mutex<_>>`**

Change imports:

```rust
use std::sync::{Arc, Mutex};
```

Change `AppState`:

```rust
pub struct AppState {
    pub settings_store: SettingsStore,
    pub settings: Arc<Mutex<LocalSettings>>,
    pub registry: Arc<Mutex<PeerRegistry>>,
    pub active_pairing: Arc<Mutex<Option<PairingSession>>>,
}
```

Expected: existing command code still locks fields with `.lock().unwrap()`.

- [x] **Step 2: Run Rust tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml commands::tests registry::tests
```

Expected: command and registry tests pass.

- [x] **Step 3: Commit shared state change**

```powershell
git add src-tauri/src/commands.rs
git commit -m "refactor: share app state with background tasks"
```

Expected: only `commands.rs` is committed.

Execution note: this refactor required matching `lib.rs` construction changes before the project could compile, so the shared state change was committed together with the discovery receiver wiring in `70d5309`.

## Task 2: Add Discovery Packet Application And Receive Loop

**Files:**
- Modify: `src-tauri/src/discovery.rs`

- [x] **Step 1: Add packet application helper**

Add imports:

```rust
use crate::{
    domain::{DeviceId, DeviceInfo},
    registry::PeerRegistry,
    protocol::{decode_message, encode_message, DiscoveryPacket, LanMessage, PROTOCOL_VERSION},
};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::{Arc, Mutex},
};
```

Add this helper after `decode_discovery`:

```rust
pub fn apply_discovery_packet(
    bytes: &[u8],
    local_device_id: &DeviceId,
    registry: &mut PeerRegistry,
) -> Result<bool> {
    let Some(device) = decode_discovery(bytes)? else {
        return Ok(false);
    };

    if device.id == *local_device_id {
        return Ok(false);
    }

    registry.mark_discovered(device);
    Ok(true)
}
```

Expected: valid remote discovery packets return `true`; self, unsupported, and non-discovery packets return `false`.

- [x] **Step 2: Add UDP receive loop**

Add this function after `announce_loop`:

```rust
pub async fn receive_loop(
    local_device_id: DeviceId,
    port: u16,
    registry: Arc<Mutex<PeerRegistry>>,
) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("failed to bind discovery UDP listener on port {port}"))?;
    let mut buffer = vec![0_u8; 64 * 1024];

    loop {
        let (len, source) = socket
            .recv_from(&mut buffer)
            .await
            .context("failed to receive discovery UDP packet")?;
        let packet = &buffer[..len];

        match registry.lock() {
            Ok(mut registry) => {
                if let Err(err) = apply_discovery_packet(packet, &local_device_id, &mut registry) {
                    tracing::debug!(?err, %source, "ignored invalid discovery packet");
                }
            }
            Err(err) => {
                tracing::error!(?err, "discovery registry lock poisoned");
                return Ok(());
            }
        }
    }
}
```

Expected: bind errors stop the loop with context; malformed packets are logged and ignored without stopping the loop.

- [x] **Step 3: Add tests for packet application**

Add tests under `discovery::tests`:

```rust
#[test]
fn apply_discovery_packet_adds_remote_device() {
    let local = DeviceInfo::new_local("Windows Desk", 45731);
    let remote = DeviceInfo::new_local("MacBook", 45731);
    let encoded = encode_discovery(remote.clone()).unwrap();
    let mut registry = PeerRegistry::new();

    let applied = apply_discovery_packet(&encoded, &local.id, &mut registry).unwrap();

    assert!(applied);
    assert_eq!(registry.discovered(), vec![remote]);
}

#[test]
fn apply_discovery_packet_ignores_local_device() {
    let local = DeviceInfo::new_local("Windows Desk", 45731);
    let encoded = encode_discovery(local.clone()).unwrap();
    let mut registry = PeerRegistry::new();

    let applied = apply_discovery_packet(&encoded, &local.id, &mut registry).unwrap();

    assert!(!applied);
    assert!(registry.discovered().is_empty());
}

#[test]
fn apply_discovery_packet_updates_paired_device_state() {
    let local = DeviceInfo::new_local("Windows Desk", 45731);
    let remote = DeviceInfo::new_local("MacBook", 45731);
    let encoded = encode_discovery(remote.clone()).unwrap();
    let mut registry = PeerRegistry::from_paired(vec![PairedPeer {
        device: remote.clone(),
        receive_clipboard: true,
        is_default_file_target: false,
        state: PeerConnectionState::Offline,
    }]);

    let applied = apply_discovery_packet(&encoded, &local.id, &mut registry).unwrap();

    assert!(applied);
    assert!(registry.discovered().is_empty());
    assert_eq!(registry.paired()[0].state, PeerConnectionState::Discovered);
}
```

Expected: tests prove UI-visible registry changes happen without the Tauri app.

- [x] **Step 4: Run discovery tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml discovery::tests registry::tests
```

Expected: discovery and registry tests pass.

- [x] **Step 5: Commit receive logic**

```powershell
git add src-tauri/src/discovery.rs
git commit -m "feat: receive LAN discovery packets"
```

Expected: discovery receive helper and tests are committed.

Execution note: `cargo test` accepts a single test filter, so verification was run as the full Rust test suite instead of using two filter arguments.

## Task 3: Wire Receive Loop Into App Startup

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [x] **Step 1: Create shared state handles before `app.manage`**

Inside setup, after `registry` is created, add:

```rust
let settings = Arc::new(Mutex::new(settings));
let registry = Arc::new(Mutex::new(registry));
let active_pairing = Arc::new(Mutex::new(None));
```

Then manage those shared handles:

```rust
app.manage(AppState {
    settings_store,
    settings: settings.clone(),
    registry: registry.clone(),
    active_pairing,
});
```

Expected: commands and background tasks share one registry.

- [x] **Step 2: Spawn receive loop**

After the existing `announce_loop` spawn, add:

```rust
let receive_device_id = discovery_device.id.clone();
let receive_registry = registry.clone();
tauri::async_runtime::spawn(async move {
    if let Err(err) =
        discovery::receive_loop(receive_device_id, discovery_port, receive_registry).await
    {
        tracing::error!(?err, "LAN discovery receiver stopped");
    }
});
```

Expected: app listens on its discovery port whenever it starts.

- [x] **Step 3: Add import for shared state**

Change imports in `lib.rs`:

```rust
use std::sync::{Arc, Mutex};
```

Expected: app compiles.

- [x] **Step 4: Run full verification**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
pnpm build
```

Expected: Rust tests and frontend build pass.

- [x] **Step 5: Run manual dev smoke**

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
pnpm tauri dev
```

Expected: app window still opens. If another LAN Cross Sync instance is running on the same LAN, it appears under `Discovered devices` within a few seconds. Stop the dev process after the smoke test.

- [x] **Step 6: Commit app wiring**

```powershell
git add src-tauri/src/lib.rs
git commit -m "feat: wire discovery receiver into app"
```

Expected: startup wiring is committed.

## Task 4: Final Verification And Review

**Files:**
- Modify: `docs/superpowers/plans/2026-07-23-discovery-receive-loop.md`

- [x] **Step 1: Mark completed plan checkboxes**

Update this file so every completed step is checked.

- [x] **Step 2: Run final verification**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
pnpm build
git status --short
```

Expected: tests/build pass and the only dirty file is this plan, or no dirty files after committing plan progress.

- [x] **Step 3: Commit plan progress**

```powershell
git add docs/superpowers/plans/2026-07-23-discovery-receive-loop.md
git commit -m "docs: add discovery receive implementation plan"
```

Expected: plan progress is committed.

- [ ] **Step 4: Request final code review**

Review the implementation range from the plan commit's parent to `HEAD`. Review must check:

- UDP receive loop is robust.
- Self-discovery is ignored.
- Unsupported protocol versions are ignored.
- Shared state is safe and commands see background updates.
- No pairing/clipboard/file-transfer scope was accidentally added.

Expected: no Critical or Important issues remain.

## Follow-Up Plans After This One

1. Full two-device pairing handshake and trusted peer exchange.
2. Automatic reconnect/connection status using paired devices seen through discovery.
3. Clipboard text/image sync with loop prevention.
4. File and folder transfer with receive-side save prompt.

## Self-Review

- Spec coverage: Covers discovery receiving and UI-visible discovered device population only. Pairing and payload transfer remain deferred as intended.
- Placeholder scan: No `TODO`, `TBD`, or unspecified implementation steps.
- Type consistency: Uses existing `DeviceId`, `DeviceInfo`, `PeerRegistry`, `PairedPeer`, and `PeerConnectionState` types.
