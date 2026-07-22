# LAN Cross Sync Connection Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first working foundation for the LAN Cross Sync Tauri app: project scaffold, local settings, LAN discovery messages, pairing state, connection registry, tray shell, autostart setting, and a control-panel UI.

**Architecture:** Use a single Tauri v2 app with a React + TypeScript Web UI and Rust-owned core logic. This plan implements only the foundation layer; clipboard payload sync and file/folder transfer are intentionally left for later plans, but the data model exposes the receive-clipboard switch and default file target needed by those later features.

**Tech Stack:** Tauri v2, Rust, Tokio, Serde, React, TypeScript, Vite, pnpm. Official docs to keep open while executing: Tauri project creation https://v2.tauri.app/start/create-project/, Tauri autostart https://v2.tauri.app/plugin/autostart/, Tauri plugin overview https://v2.tauri.app/plugin/, Tauri filesystem security notes https://v2.tauri.app/plugin/file-system/.

---

## Scope Check

The approved spec covers several subsystems: discovery, pairing, transport, clipboard sync, file/folder transfer, tray/autostart, permissions, logging, and tests. This plan deliberately implements the first subsystem slice only: the connection foundation. It must produce working, testable software on its own:

- A Tauri app opens a control panel.
- A local device identity is created and persisted.
- Paired device records are persisted.
- A 6-digit pairing flow exists at the state-machine level and through Tauri commands.
- LAN discovery packets can be encoded/decoded and sent periodically.
- The UI can show local device info, discovered devices, paired devices, and connection state.
- The UI can toggle per-device receive-clipboard and default file target settings.
- The app can run from the tray and expose an autostart switch.

Clipboard payload sync and file/folder transfer get their own follow-up plans after this foundation passes manual two-machine testing.

## File Structure

Create or modify these files:

- `package.json`: frontend scripts and Tauri CLI script.
- `src/App.tsx`: main control-panel UI.
- `src/App.css`: compact desktop-app layout.
- `src/main.tsx`: React entry point.
- `src/lib/api.ts`: typed Tauri command wrappers.
- `src/lib/types.ts`: frontend DTO types matching Rust command responses.
- `src-tauri/Cargo.toml`: Rust dependencies and Tauri plugins.
- `src-tauri/tauri.conf.json`: app identity, window, tray/bundle config.
- `src-tauri/capabilities/default.json`: autostart permission grants.
- `src-tauri/src/lib.rs`: Tauri builder, managed app state, command registration, tray setup.
- `src-tauri/src/main.rs`: desktop entry point.
- `src-tauri/src/domain.rs`: device IDs, device info, peer records, settings DTOs.
- `src-tauri/src/settings.rs`: JSON-backed local settings store.
- `src-tauri/src/protocol.rs`: versioned discovery and pairing message DTOs.
- `src-tauri/src/pairing.rs`: 6-digit pairing code and pairing session state machine.
- `src-tauri/src/discovery.rs`: UDP discovery packet encoding/decoding and background announcer.
- `src-tauri/src/registry.rs`: in-memory discovered/paired/connection status registry.
- `src-tauri/src/commands.rs`: Tauri command functions used by the UI.
- `src-tauri/src/error.rs`: app error type converted into UI-safe strings.

## Task 1: Scaffold The Tauri App And Repository

**Files:**
- Create: generated Tauri/Vite files under project root
- Modify: `package.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Initialize git**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git init
git add docs/superpowers/specs/2026-07-23-lan-cross-device-sync-design.md docs/superpowers/plans/2026-07-23-connection-foundation.md
git commit -m "docs: add LAN cross sync design and foundation plan"
```

Expected: a new repository is created and the approved design/plan are committed.

- [ ] **Step 2: Scaffold the Tauri project**

Run:

```powershell
cd C:\A-my\lan-cross-sync
pnpm create tauri-app@latest .
```

Answer the prompts exactly:

```text
Directory is not empty: Yes, continue
Project name: lan-cross-sync
Identifier: com.local.lancrosssync
Choose which language to use for your frontend: TypeScript / JavaScript
Choose your package manager: pnpm
Choose your UI template: React
Choose your UI flavor: TypeScript
```

Expected: `package.json`, `src`, and `src-tauri` are created without deleting the existing `docs` directory.

- [ ] **Step 3: Install dependencies**

Run:

```powershell
cd C:\A-my\lan-cross-sync
pnpm install
pnpm tauri add autostart
cargo add serde --features derive --manifest-path src-tauri/Cargo.toml
cargo add serde_json --manifest-path src-tauri/Cargo.toml
cargo add tokio --features full --manifest-path src-tauri/Cargo.toml
cargo add uuid --features v4,serde --manifest-path src-tauri/Cargo.toml
cargo add thiserror --manifest-path src-tauri/Cargo.toml
cargo add anyhow --manifest-path src-tauri/Cargo.toml
cargo add directories --manifest-path src-tauri/Cargo.toml
cargo add time --features serde,formatting --manifest-path src-tauri/Cargo.toml
cargo add tracing --manifest-path src-tauri/Cargo.toml
cargo add rand --manifest-path src-tauri/Cargo.toml
cargo add tempfile --dev --manifest-path src-tauri/Cargo.toml
```

Expected: frontend dependencies install, the Tauri autostart plugin is added, and Rust dependencies are written to `src-tauri/Cargo.toml`.

- [ ] **Step 4: Verify baseline project builds**

Run:

```powershell
cd C:\A-my\lan-cross-sync
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: frontend build succeeds and Rust tests pass.

- [ ] **Step 5: Commit scaffold**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add package.json pnpm-lock.yaml src src-tauri
git commit -m "chore: scaffold Tauri app"
```

Expected: scaffold and dependency files are committed.

## Task 2: Add Core Domain Types

**Files:**
- Create: `src-tauri/src/domain.rs`

- [ ] **Step 1: Write domain type tests**

Create `src-tauri/src/domain.rs` with this initial test-focused content:

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub Uuid);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub app_version: String,
    pub protocol_version: u16,
    pub port: u16,
    pub capabilities: Vec<Capability>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Discovery,
    Pairing,
    Clipboard,
    FileTransfer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerConnectionState {
    Offline,
    Discovered,
    Pairing,
    Connected,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PairedPeer {
    pub device: DeviceInfo,
    pub receive_clipboard: bool,
    pub is_default_file_target: bool,
    pub state: PeerConnectionState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalSettings {
    pub local_device: DeviceInfo,
    pub paired_peers: Vec<PairedPeer>,
    pub autostart_enabled: bool,
}

impl DeviceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl DeviceInfo {
    pub fn new_local(name: impl Into<String>, port: u16) -> Self {
        Self {
            id: DeviceId::new(),
            name: name.into(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: 1,
            port,
            capabilities: vec![
                Capability::Discovery,
                Capability::Pairing,
                Capability::Clipboard,
                Capability::FileTransfer,
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_device_has_expected_capabilities() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);

        assert_eq!(device.name, "Windows Desk");
        assert_eq!(device.protocol_version, 1);
        assert_eq!(device.port, 45731);
        assert!(device.capabilities.contains(&Capability::Discovery));
        assert!(device.capabilities.contains(&Capability::Pairing));
        assert!(device.capabilities.contains(&Capability::Clipboard));
        assert!(device.capabilities.contains(&Capability::FileTransfer));
    }

    #[test]
    fn paired_peer_defaults_can_be_serialized() {
        let peer = PairedPeer {
            device: DeviceInfo::new_local("MacBook", 45731),
            receive_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Offline,
        };

        let json = serde_json::to_string(&peer).unwrap();
        let decoded: PairedPeer = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, peer);
    }
}
```

- [ ] **Step 2: Run domain tests**

Run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml domain::tests -- --nocapture
```

Expected: tests pass.

- [ ] **Step 3: Expose the domain module**

Modify `src-tauri/src/lib.rs` so it includes this module declaration near the top:

```rust
mod domain;
```

- [ ] **Step 4: Run Rust tests**

Run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all Rust tests pass.

- [ ] **Step 5: Commit domain types**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add src-tauri/src/domain.rs src-tauri/src/lib.rs
git commit -m "feat: add core device domain types"
```

Expected: domain model is committed.

## Task 3: Add JSON Settings Persistence

**Files:**
- Create: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create the settings store**

Create `src-tauri/src/settings.rs`:

```rust
use crate::domain::{DeviceInfo, LocalSettings, PairedPeer};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const DEFAULT_DISCOVERY_PORT: u16 = 45731;

#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load_or_create(&self, device_name: &str) -> Result<LocalSettings> {
        if self.path.exists() {
            let raw = fs::read_to_string(&self.path)
                .with_context(|| format!("failed to read settings file {}", self.path.display()))?;
            let settings = serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse settings file {}", self.path.display()))?;
            return Ok(settings);
        }

        let settings = LocalSettings {
            local_device: DeviceInfo::new_local(device_name, DEFAULT_DISCOVERY_PORT),
            paired_peers: Vec::new(),
            autostart_enabled: true,
        };
        self.save(&settings)?;
        Ok(settings)
    }

    pub fn save(&self, settings: &LocalSettings) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create settings directory {}", parent.display()))?;
        }

        let raw = serde_json::to_string_pretty(settings).context("failed to serialize settings")?;
        fs::write(&self.path, raw)
            .with_context(|| format!("failed to write settings file {}", self.path.display()))?;
        Ok(())
    }

    pub fn add_or_update_peer(&self, peer: PairedPeer) -> Result<LocalSettings> {
        let mut settings = self.load_or_create("LAN Cross Sync")?;
        settings.paired_peers.retain(|p| p.device.id != peer.device.id);
        settings.paired_peers.push(peer);
        self.save(&settings)?;
        Ok(settings)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PeerConnectionState, PairedPeer};

    #[test]
    fn load_or_create_persists_default_settings() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));

        let settings = store.load_or_create("Windows Desk").unwrap();
        let loaded = store.load_or_create("Ignored Name").unwrap();

        assert_eq!(settings, loaded);
        assert_eq!(loaded.local_device.name, "Windows Desk");
        assert!(loaded.autostart_enabled);
        assert!(store.path().exists());
    }

    #[test]
    fn add_or_update_peer_replaces_existing_device() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let device = DeviceInfo::new_local("MacBook", DEFAULT_DISCOVERY_PORT);

        let first = PairedPeer {
            device: device.clone(),
            receive_clipboard: false,
            is_default_file_target: false,
            state: PeerConnectionState::Offline,
        };
        let second = PairedPeer {
            device,
            receive_clipboard: true,
            is_default_file_target: true,
            state: PeerConnectionState::Connected,
        };

        store.add_or_update_peer(first).unwrap();
        let settings = store.add_or_update_peer(second).unwrap();

        assert_eq!(settings.paired_peers.len(), 1);
        assert!(settings.paired_peers[0].receive_clipboard);
        assert!(settings.paired_peers[0].is_default_file_target);
        assert_eq!(settings.paired_peers[0].state, PeerConnectionState::Connected);
    }
}
```

- [ ] **Step 2: Expose the settings module**

Modify `src-tauri/src/lib.rs`:

```rust
mod domain;
mod settings;
```

- [ ] **Step 3: Run settings tests**

Run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml settings::tests -- --nocapture
```

Expected: settings persistence tests pass.

- [ ] **Step 4: Commit settings store**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add src-tauri/src/settings.rs src-tauri/src/lib.rs
git commit -m "feat: persist local device settings"
```

Expected: settings store is committed.

## Task 4: Add Protocol Messages

**Files:**
- Create: `src-tauri/src/protocol.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create protocol DTOs and tests**

Create `src-tauri/src/protocol.rs`:

```rust
use crate::domain::{DeviceId, DeviceInfo};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LanMessage {
    Discovery(DiscoveryPacket),
    PairingRequest(PairingRequest),
    PairingResponse(PairingResponse),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryPacket {
    pub protocol_version: u16,
    pub device: DeviceInfo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PairingRequest {
    pub session_id: String,
    pub from_device: DeviceInfo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PairingResponse {
    pub session_id: String,
    pub accepted: bool,
    pub from_device_id: DeviceId,
    pub reason: Option<String>,
}

impl DiscoveryPacket {
    pub fn new(device: DeviceInfo) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            device,
        }
    }
}

pub fn encode_message(message: &LanMessage) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(message)
}

pub fn decode_message(bytes: &[u8]) -> Result<LanMessage, serde_json::Error> {
    serde_json::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DeviceInfo;

    #[test]
    fn discovery_packet_round_trips() {
        let device = DeviceInfo::new_local("MacBook", 45731);
        let message = LanMessage::Discovery(DiscoveryPacket::new(device.clone()));

        let encoded = encode_message(&message).unwrap();
        let decoded = decode_message(&encoded).unwrap();

        assert_eq!(decoded, message);
    }

    #[test]
    fn pairing_response_can_reject_with_reason() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let response = LanMessage::PairingResponse(PairingResponse {
            session_id: "abc123".to_string(),
            accepted: false,
            from_device_id: device.id,
            reason: Some("invalid code".to_string()),
        });

        let encoded = encode_message(&response).unwrap();
        let decoded = decode_message(&encoded).unwrap();

        assert_eq!(decoded, response);
    }
}
```

- [ ] **Step 2: Expose protocol module**

Modify `src-tauri/src/lib.rs`:

```rust
mod domain;
mod protocol;
mod settings;
```

- [ ] **Step 3: Run protocol tests**

Run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml protocol::tests -- --nocapture
```

Expected: protocol tests pass.

- [ ] **Step 4: Commit protocol messages**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add src-tauri/src/protocol.rs src-tauri/src/lib.rs
git commit -m "feat: add LAN protocol messages"
```

Expected: protocol module is committed.

## Task 5: Add Pairing State Machine

**Files:**
- Create: `src-tauri/src/pairing.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create pairing implementation and tests**

Create `src-tauri/src/pairing.rs`:

```rust
use crate::domain::DeviceInfo;
use rand::Rng;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const PAIRING_CODE_TTL: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct PairingSession {
    pub session_id: String,
    pub code: String,
    pub local_device: DeviceInfo,
    created_at: Instant,
}

impl PairingSession {
    pub fn new(local_device: DeviceInfo) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            code: generate_pairing_code(),
            local_device,
            created_at: Instant::now(),
        }
    }

    pub fn with_code_for_test(local_device: DeviceInfo, code: impl Into<String>) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            code: code.into(),
            local_device,
            created_at: Instant::now(),
        }
    }

    pub fn verify_code(&self, candidate: &str) -> bool {
        !self.is_expired() && self.code == candidate
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > PAIRING_CODE_TTL
    }
}

pub fn generate_pairing_code() -> String {
    let code = rand::thread_rng().gen_range(0..=999_999);
    format!("{code:06}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_code_is_six_digits() {
        let code = generate_pairing_code();

        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn session_accepts_matching_code() {
        let device = DeviceInfo::new_local("MacBook", 45731);
        let session = PairingSession::with_code_for_test(device, "123456");

        assert!(session.verify_code("123456"));
        assert!(!session.verify_code("654321"));
    }
}
```

- [ ] **Step 2: Expose pairing module**

Modify `src-tauri/src/lib.rs`:

```rust
mod domain;
mod pairing;
mod protocol;
mod settings;
```

- [ ] **Step 3: Run pairing tests**

Run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml pairing::tests -- --nocapture
```

Expected: pairing tests pass.

- [ ] **Step 4: Commit pairing state machine**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add src-tauri/src/pairing.rs src-tauri/src/lib.rs
git commit -m "feat: add pairing state machine"
```

Expected: pairing module is committed.

## Task 6: Add Registry For Discovered And Paired Devices

**Files:**
- Create: `src-tauri/src/registry.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create registry implementation and tests**

Create `src-tauri/src/registry.rs`:

```rust
use crate::domain::{DeviceId, DeviceInfo, PairedPeer, PeerConnectionState};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct PeerRegistry {
    discovered: HashMap<DeviceId, DeviceInfo>,
    paired: HashMap<DeviceId, PairedPeer>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_discovered(&mut self, device: DeviceInfo) {
        if !self.paired.contains_key(&device.id) {
            self.discovered.insert(device.id.clone(), device);
        } else if let Some(peer) = self.paired.get_mut(&device.id) {
            peer.device = device;
            peer.state = PeerConnectionState::Discovered;
        }
    }

    pub fn set_paired(&mut self, peer: PairedPeer) {
        self.discovered.remove(&peer.device.id);
        self.paired.insert(peer.device.id.clone(), peer);
    }

    pub fn set_state(&mut self, id: &DeviceId, state: PeerConnectionState) {
        if let Some(peer) = self.paired.get_mut(id) {
            peer.state = state;
        }
    }

    pub fn discovered(&self) -> Vec<DeviceInfo> {
        self.discovered.values().cloned().collect()
    }

    pub fn paired(&self) -> Vec<PairedPeer> {
        self.paired.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_device_moves_to_paired() {
        let mut registry = PeerRegistry::new();
        let device = DeviceInfo::new_local("MacBook", 45731);

        registry.mark_discovered(device.clone());
        assert_eq!(registry.discovered().len(), 1);

        registry.set_paired(PairedPeer {
            device,
            receive_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        });

        assert!(registry.discovered().is_empty());
        assert_eq!(registry.paired().len(), 1);
        assert_eq!(registry.paired()[0].state, PeerConnectionState::Connected);
    }

    #[test]
    fn paired_device_discovery_updates_state() {
        let mut registry = PeerRegistry::new();
        let device = DeviceInfo::new_local("MacBook", 45731);
        registry.set_paired(PairedPeer {
            device: device.clone(),
            receive_clipboard: false,
            is_default_file_target: false,
            state: PeerConnectionState::Offline,
        });

        registry.mark_discovered(device);

        assert_eq!(registry.paired()[0].state, PeerConnectionState::Discovered);
    }
}
```

- [ ] **Step 2: Expose registry module**

Modify `src-tauri/src/lib.rs`:

```rust
mod domain;
mod pairing;
mod protocol;
mod registry;
mod settings;
```

- [ ] **Step 3: Run registry tests**

Run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml registry::tests -- --nocapture
```

Expected: registry tests pass.

- [ ] **Step 4: Commit registry**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add src-tauri/src/registry.rs src-tauri/src/lib.rs
git commit -m "feat: track discovered and paired devices"
```

Expected: registry module is committed.

## Task 7: Add Discovery Packet Service

**Files:**
- Create: `src-tauri/src/discovery.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create discovery helpers and tests**

Create `src-tauri/src/discovery.rs`:

```rust
use crate::{
    domain::DeviceInfo,
    protocol::{decode_message, encode_message, DiscoveryPacket, LanMessage},
};
use anyhow::{Context, Result};
use std::net::{Ipv4Addr, SocketAddrV4};
use tokio::{net::UdpSocket, time::{self, Duration}};

pub const DISCOVERY_BROADCAST_ADDR: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);
pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(3);

pub fn discovery_socket_addr(port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(DISCOVERY_BROADCAST_ADDR, port)
}

pub fn encode_discovery(device: DeviceInfo) -> Result<Vec<u8>> {
    let message = LanMessage::Discovery(DiscoveryPacket::new(device));
    encode_message(&message).context("failed to encode discovery packet")
}

pub fn decode_discovery(bytes: &[u8]) -> Result<Option<DeviceInfo>> {
    match decode_message(bytes).context("failed to decode LAN message")? {
        LanMessage::Discovery(packet) => Ok(Some(packet.device)),
        _ => Ok(None),
    }
}

pub async fn announce_loop(device: DeviceInfo, port: u16) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    socket.set_broadcast(true)?;
    let target = discovery_socket_addr(port);
    let payload = encode_discovery(device)?;
    let mut interval = time::interval(DISCOVERY_INTERVAL);

    loop {
        interval.tick().await;
        socket.send_to(&payload, target).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_packet_round_trips_to_device_info() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let encoded = encode_discovery(device.clone()).unwrap();
        let decoded = decode_discovery(&encoded).unwrap();

        assert_eq!(decoded, Some(device));
    }

    #[test]
    fn broadcast_address_uses_selected_port() {
        let addr = discovery_socket_addr(45731);

        assert_eq!(*addr.ip(), DISCOVERY_BROADCAST_ADDR);
        assert_eq!(addr.port(), 45731);
    }
}
```

- [ ] **Step 2: Expose discovery module**

Modify `src-tauri/src/lib.rs`:

```rust
mod discovery;
mod domain;
mod pairing;
mod protocol;
mod registry;
mod settings;
```

- [ ] **Step 3: Run discovery tests**

Run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml discovery::tests -- --nocapture
```

Expected: discovery tests pass.

- [ ] **Step 4: Commit discovery helpers**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add src-tauri/src/discovery.rs src-tauri/src/lib.rs
git commit -m "feat: add LAN discovery packets"
```

Expected: discovery helpers are committed.

## Task 8: Add UI-Safe Errors And App State Commands

**Files:**
- Create: `src-tauri/src/error.rs`
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create UI-safe error type**

Create `src-tauri/src/error.rs`:

```rust
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

#[derive(Debug, Serialize)]
pub struct UiError {
    pub message: String,
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        UiError {
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;
```

- [ ] **Step 2: Create Tauri command layer**

Create `src-tauri/src/commands.rs`:

```rust
use crate::{
    domain::{DeviceId, LocalSettings, PairedPeer, PeerConnectionState},
    error::{AppError, AppResult},
    pairing::PairingSession,
    registry::PeerRegistry,
    settings::SettingsStore,
};
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub settings_store: SettingsStore,
    pub settings: Mutex<LocalSettings>,
    pub registry: Mutex<PeerRegistry>,
    pub active_pairing: Mutex<Option<PairingSession>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DashboardState {
    pub settings: LocalSettings,
    pub discovered_devices: Vec<crate::domain::DeviceInfo>,
    pub paired_devices: Vec<PairedPeer>,
    pub active_pairing_code: Option<String>,
}

#[tauri::command]
pub fn get_dashboard_state(state: State<'_, AppState>) -> AppResult<DashboardState> {
    let settings = state.settings.lock().unwrap().clone();
    let registry = state.registry.lock().unwrap();
    let active_pairing_code = state
        .active_pairing
        .lock()
        .unwrap()
        .as_ref()
        .map(|session| session.code.clone());

    Ok(DashboardState {
        paired_devices: settings.paired_peers.clone(),
        settings,
        discovered_devices: registry.discovered(),
        active_pairing_code,
    })
}

#[tauri::command]
pub fn start_pairing(state: State<'_, AppState>) -> AppResult<String> {
    let local_device = state.settings.lock().unwrap().local_device.clone();
    let session = PairingSession::new(local_device);
    let code = session.code.clone();
    *state.active_pairing.lock().unwrap() = Some(session);
    Ok(code)
}

#[tauri::command]
pub fn cancel_pairing(state: State<'_, AppState>) -> AppResult<()> {
    *state.active_pairing.lock().unwrap() = None;
    Ok(())
}

#[tauri::command]
pub fn set_receive_clipboard(
    state: State<'_, AppState>,
    device_id: DeviceId,
    enabled: bool,
) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    let peer = settings
        .paired_peers
        .iter_mut()
        .find(|peer| peer.device.id == device_id)
        .ok_or_else(|| AppError::Message("Paired device not found.".to_string()))?;
    peer.receive_clipboard = enabled;
    state.settings_store.save(&settings)?;
    Ok(settings.clone())
}

#[tauri::command]
pub fn set_default_file_target(
    state: State<'_, AppState>,
    device_id: DeviceId,
) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    let mut found = false;
    for peer in &mut settings.paired_peers {
        let is_target = peer.device.id == device_id;
        peer.is_default_file_target = is_target;
        found |= is_target;
    }
    if !found {
        return Err(AppError::Message("Paired device not found.".to_string()));
    }
    state.settings_store.save(&settings)?;
    Ok(settings.clone())
}

#[tauri::command]
pub fn clear_pairing(state: State<'_, AppState>, device_id: DeviceId) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    settings.paired_peers.retain(|peer| peer.device.id != device_id);
    state.settings_store.save(&settings)?;
    state
        .registry
        .lock()
        .unwrap()
        .set_state(&device_id, PeerConnectionState::Offline);
    Ok(settings.clone())
}
```

- [ ] **Step 3: Wire modules and commands**

Modify `src-tauri/src/lib.rs` to include the new modules and command registration. Preserve scaffolded plugin setup and merge this shape into the generated file:

```rust
mod commands;
mod discovery;
mod domain;
mod error;
mod pairing;
mod protocol;
mod registry;
mod settings;

use commands::{
    cancel_pairing, clear_pairing, get_dashboard_state, set_default_file_target,
    set_receive_clipboard, start_pairing, AppState,
};
use registry::PeerRegistry;
use settings::SettingsStore;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let app_config = app
                .path()
                .app_config_dir()
                .expect("failed to resolve app config directory");
            let settings_store = SettingsStore::new(app_config.join("settings.json"));
            let settings = settings_store
                .load_or_create("LAN Cross Sync")
                .expect("failed to load settings");

            app.manage(AppState {
                settings_store,
                settings: Mutex::new(settings),
                registry: Mutex::new(PeerRegistry::new()),
                active_pairing: Mutex::new(None),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard_state,
            start_pairing,
            cancel_pairing,
            set_receive_clipboard,
            set_default_file_target,
            clear_pairing
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: Run Rust tests and build**

Run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
```

Expected: Rust tests and frontend build pass.

- [ ] **Step 5: Commit command layer**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add src-tauri/src/error.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: expose connection foundation commands"
```

Expected: Tauri command layer is committed.

## Task 9: Build The Control Panel UI

**Files:**
- Create: `src/lib/types.ts`
- Create: `src/lib/api.ts`
- Modify: `src/App.tsx`
- Modify: `src/App.css`

- [ ] **Step 1: Add frontend DTO types**

Create `src/lib/types.ts`:

```ts
export type DeviceId = string

export interface DeviceInfo {
  id: DeviceId
  name: string
  app_version: string
  protocol_version: number
  port: number
  capabilities: string[]
}

export type PeerConnectionState = 'offline' | 'discovered' | 'pairing' | 'connected' | 'error'

export interface PairedPeer {
  device: DeviceInfo
  receive_clipboard: boolean
  is_default_file_target: boolean
  state: PeerConnectionState
}

export interface LocalSettings {
  local_device: DeviceInfo
  paired_peers: PairedPeer[]
  autostart_enabled: boolean
}

export interface DashboardState {
  settings: LocalSettings
  discovered_devices: DeviceInfo[]
  paired_devices: PairedPeer[]
  active_pairing_code: string | null
}
```

- [ ] **Step 2: Add Tauri API wrapper**

Create `src/lib/api.ts`:

```ts
import { invoke } from '@tauri-apps/api/core'
import type { DashboardState, DeviceId, LocalSettings } from './types'

export function getDashboardState(): Promise<DashboardState> {
  return invoke('get_dashboard_state')
}

export function startPairing(): Promise<string> {
  return invoke('start_pairing')
}

export function cancelPairing(): Promise<void> {
  return invoke('cancel_pairing')
}

export function setReceiveClipboard(deviceId: DeviceId, enabled: boolean): Promise<LocalSettings> {
  return invoke('set_receive_clipboard', { deviceId, enabled })
}

export function setDefaultFileTarget(deviceId: DeviceId): Promise<LocalSettings> {
  return invoke('set_default_file_target', { deviceId })
}

export function clearPairing(deviceId: DeviceId): Promise<LocalSettings> {
  return invoke('clear_pairing', { deviceId })
}
```

- [ ] **Step 3: Replace the app UI**

Replace `src/App.tsx`:

```tsx
import { useEffect, useState } from 'react'
import './App.css'
import {
  cancelPairing,
  clearPairing,
  getDashboardState,
  setDefaultFileTarget,
  setReceiveClipboard,
  startPairing,
} from './lib/api'
import type { DashboardState, DeviceId, PairedPeer } from './lib/types'

function deviceIdText(id: DeviceId): string {
  return id
}

function PeerCard({ peer, refresh }: { peer: PairedPeer; refresh: () => Promise<void> }) {
  async function toggleClipboard() {
    await setReceiveClipboard(peer.device.id, !peer.receive_clipboard)
    await refresh()
  }

  async function makeDefaultTarget() {
    await setDefaultFileTarget(peer.device.id)
    await refresh()
  }

  async function removePairing() {
    await clearPairing(peer.device.id)
    await refresh()
  }

  return (
    <article className="peer-card">
      <div>
        <h3>{peer.device.name}</h3>
        <p>{peer.state}</p>
        <code>{deviceIdText(peer.device.id)}</code>
      </div>
      <div className="peer-actions">
        <label>
          <input type="checkbox" checked={peer.receive_clipboard} onChange={toggleClipboard} />
          Receive clipboard
        </label>
        <button onClick={makeDefaultTarget} disabled={peer.is_default_file_target}>
          {peer.is_default_file_target ? 'Default target' : 'Set file target'}
        </button>
        <button className="danger" onClick={removePairing}>
          Clear pairing
        </button>
      </div>
    </article>
  )
}

export default function App() {
  const [dashboard, setDashboard] = useState<DashboardState | null>(null)
  const [error, setError] = useState<string | null>(null)

  async function refresh() {
    try {
      setDashboard(await getDashboardState())
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load dashboard state.')
    }
  }

  async function beginPairing() {
    try {
      await startPairing()
      await refresh()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to start pairing.')
    }
  }

  async function stopPairing() {
    await cancelPairing()
    await refresh()
  }

  useEffect(() => {
    void refresh()
    const timer = window.setInterval(() => void refresh(), 2_000)
    return () => window.clearInterval(timer)
  }, [])

  if (!dashboard) {
    return <main className="shell">Loading...</main>
  }

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <h1>LAN Cross Sync</h1>
          <p>{dashboard.settings.local_device.name}</p>
        </div>
        <button onClick={() => void refresh()}>Refresh</button>
      </header>

      {error && <section className="error">{error}</section>}

      <section className="panel">
        <div className="panel-title">
          <h2>Pairing</h2>
          {dashboard.active_pairing_code ? (
            <button onClick={() => void stopPairing()}>Cancel</button>
          ) : (
            <button onClick={() => void beginPairing()}>Start pairing</button>
          )}
        </div>
        {dashboard.active_pairing_code ? (
          <div className="pairing-code">{dashboard.active_pairing_code}</div>
        ) : (
          <p>No active pairing session.</p>
        )}
      </section>

      <section className="panel">
        <h2>Paired devices</h2>
        {dashboard.paired_devices.length === 0 ? (
          <p>No paired devices yet.</p>
        ) : (
          <div className="peer-list">
            {dashboard.paired_devices.map((peer) => (
              <PeerCard key={deviceIdText(peer.device.id)} peer={peer} refresh={refresh} />
            ))}
          </div>
        )}
      </section>

      <section className="panel">
        <h2>Discovered devices</h2>
        {dashboard.discovered_devices.length === 0 ? (
          <p>No unpaired devices discovered.</p>
        ) : (
          <div className="peer-list">
            {dashboard.discovered_devices.map((device) => (
              <article className="peer-card" key={deviceIdText(device.id)}>
                <div>
                  <h3>{device.name}</h3>
                  <p>Available to pair</p>
                  <code>{deviceIdText(device.id)}</code>
                </div>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="drop-zone" aria-label="Future file drop zone">
        <strong>File drop area</strong>
        <span>Foundation only. File transfer comes in the next implementation plan.</span>
      </section>
    </main>
  )
}
```

- [ ] **Step 4: Replace app styles**

Replace `src/App.css`:

```css
:root {
  color: #1d2430;
  background: #f4f6f8;
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

button {
  border: 1px solid #cbd3dc;
  border-radius: 6px;
  background: #ffffff;
  color: #1d2430;
  cursor: pointer;
  font: inherit;
  padding: 8px 12px;
}

button:hover {
  background: #eef3f8;
}

button:disabled {
  color: #6b7785;
  cursor: default;
}

.shell {
  box-sizing: border-box;
  min-height: 100vh;
  padding: 24px;
}

.topbar,
.panel-title,
.peer-card {
  align-items: center;
  display: flex;
  justify-content: space-between;
  gap: 16px;
}

.topbar h1,
.panel h2,
.peer-card h3,
.topbar p,
.peer-card p {
  margin: 0;
}

.topbar {
  margin-bottom: 20px;
}

.topbar p,
.peer-card p,
.panel p,
.drop-zone span {
  color: #5f6b7a;
}

.panel,
.drop-zone,
.error {
  background: #ffffff;
  border: 1px solid #dbe2ea;
  border-radius: 8px;
  margin-bottom: 16px;
  padding: 16px;
}

.error {
  border-color: #e29a9a;
  color: #9b1c1c;
}

.pairing-code {
  border: 1px dashed #8593a3;
  border-radius: 8px;
  font-size: 36px;
  font-weight: 700;
  letter-spacing: 4px;
  margin-top: 12px;
  padding: 18px;
  text-align: center;
}

.peer-list {
  display: grid;
  gap: 10px;
}

.peer-card {
  border: 1px solid #e0e6ed;
  border-radius: 8px;
  padding: 12px;
}

.peer-card code {
  color: #607086;
  display: block;
  font-size: 12px;
  margin-top: 4px;
  max-width: 420px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.peer-actions {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  justify-content: flex-end;
}

.peer-actions label {
  align-items: center;
  display: flex;
  gap: 6px;
}

.danger {
  border-color: #e1b7b7;
  color: #9b1c1c;
}

.drop-zone {
  align-items: center;
  border-style: dashed;
  display: flex;
  flex-direction: column;
  gap: 6px;
  justify-content: center;
  min-height: 120px;
}
```

- [ ] **Step 5: Build frontend**

Run:

```powershell
cd C:\A-my\lan-cross-sync
pnpm build
```

Expected: TypeScript and Vite build pass.

- [ ] **Step 6: Commit control panel UI**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add src/lib/types.ts src/lib/api.ts src/App.tsx src/App.css
git commit -m "feat: add connection control panel"
```

Expected: UI is committed.

## Task 10: Add Tray And Autostart Controls

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src/lib/api.ts`
- Modify: `src/App.tsx`

- [ ] **Step 1: Add autostart commands in Rust**

Modify `src-tauri/src/commands.rs` by adding these imports and commands:

```rust
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub fn get_autostart_enabled(app: tauri::AppHandle) -> AppResult<bool> {
    app.autolaunch()
        .is_enabled()
        .map_err(|err| AppError::Message(format!("Failed to read autostart state: {err}")))
}

#[tauri::command]
pub fn set_autostart_enabled(app: tauri::AppHandle, enabled: bool) -> AppResult<bool> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };

    result.map_err(|err| AppError::Message(format!("Failed to update autostart: {err}")))?;
    manager
        .is_enabled()
        .map_err(|err| AppError::Message(format!("Failed to read autostart state: {err}")))
}
```

Then register them in `src-tauri/src/lib.rs`:

```rust
use commands::{
    cancel_pairing, clear_pairing, get_autostart_enabled, get_dashboard_state,
    set_autostart_enabled, set_default_file_target, set_receive_clipboard, start_pairing, AppState,
};
```

And add them to `tauri::generate_handler!`:

```rust
get_autostart_enabled,
set_autostart_enabled
```

- [ ] **Step 2: Add autostart permissions**

Ensure `src-tauri/capabilities/default.json` contains these permissions:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default permissions for LAN Cross Sync",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "autostart:allow-enable",
    "autostart:allow-disable",
    "autostart:allow-is-enabled"
  ]
}
```

- [ ] **Step 3: Add frontend autostart wrappers**

Modify `src/lib/api.ts`:

```ts
export function getAutostartEnabled(): Promise<boolean> {
  return invoke('get_autostart_enabled')
}

export function setAutostartEnabled(enabled: boolean): Promise<boolean> {
  return invoke('set_autostart_enabled', { enabled })
}
```

- [ ] **Step 4: Add the UI switch**

Modify `src/App.tsx` imports:

```tsx
import {
  cancelPairing,
  clearPairing,
  getAutostartEnabled,
  getDashboardState,
  setAutostartEnabled,
  setDefaultFileTarget,
  setReceiveClipboard,
  startPairing,
} from './lib/api'
```

Add state inside `App`:

```tsx
const [autostart, setAutostart] = useState<boolean | null>(null)
```

Update `refresh()` to load it:

```tsx
setDashboard(await getDashboardState())
setAutostart(await getAutostartEnabled())
```

Add this function inside `App`:

```tsx
async function toggleAutostart() {
  const next = await setAutostartEnabled(!autostart)
  setAutostart(next)
}
```

Add this panel before the drop zone:

```tsx
<section className="panel">
  <h2>Startup</h2>
  <label className="setting-row">
    <input
      type="checkbox"
      checked={Boolean(autostart)}
      onChange={() => void toggleAutostart()}
    />
    Start LAN Cross Sync when this computer starts
  </label>
</section>
```

Add this style to `src/App.css`:

```css
.setting-row {
  align-items: center;
  display: flex;
  gap: 8px;
}
```

- [ ] **Step 5: Run verification**

Run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
```

Expected: Rust tests and frontend build pass.

- [ ] **Step 6: Commit tray/autostart controls**

Run:

```powershell
cd C:\A-my\lan-cross-sync
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/capabilities/default.json src/lib/api.ts src/App.tsx src/App.css
git commit -m "feat: add autostart control"
```

Expected: autostart commands and UI are committed.

## Task 11: Manual Run And Foundation Acceptance

**Files:**
- Modify: `docs/superpowers/plans/2026-07-23-connection-foundation.md` only if execution notes need correction

- [ ] **Step 1: Start the dev app**

Run:

```powershell
cd C:\A-my\lan-cross-sync
pnpm tauri dev
```

Expected: Tauri opens the app window and the UI shows:

```text
LAN Cross Sync
Pairing
Paired devices
Discovered devices
File drop area
Startup
```

- [ ] **Step 2: Verify local settings were created**

Run this in a second PowerShell terminal:

```powershell
Get-ChildItem "$env:APPDATA\\com.local.lancrosssync" -Recurse -ErrorAction SilentlyContinue
```

Expected: a `settings.json` file exists somewhere under the app config directory. If Windows resolves the Tauri config directory differently, use the app logs or debugger to inspect `app.path().app_config_dir()`.

- [ ] **Step 3: Verify pairing code behavior**

In the app:

1. Click `Start pairing`.
2. Confirm a 6-digit code appears.
3. Click `Cancel`.
4. Confirm the code disappears.

Expected: pairing code appears and clears without app restart.

- [ ] **Step 4: Verify build commands**

Stop the dev app and run:

```powershell
cd C:\A-my\lan-cross-sync
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
```

Expected: all tests pass and frontend build succeeds.

- [ ] **Step 5: Commit acceptance notes if any plan corrections were required**

If no plan corrections were needed, skip this commit. If the plan was corrected during execution, run:

```powershell
cd C:\A-my\lan-cross-sync
git add docs/superpowers/plans/2026-07-23-connection-foundation.md
git commit -m "docs: update foundation implementation notes"
```

Expected: only real plan corrections are committed.

## Follow-Up Plans After This One

After this plan is implemented and accepted, write separate plans for:

1. LAN receive loop and actual discovered-device population.
2. Full two-device pairing handshake and trusted peer exchange.
3. Clipboard text/image sync with loop prevention.
4. File and folder transfer with receive-side save prompt.
5. Packaging, tray polish, logs export, and cross-platform acceptance tests.

## Self-Review

- Spec coverage: This plan covers project scaffold, Tauri architecture, settings, discovery packet format, pairing code state, paired/discovered registry, UI control panel, receive-clipboard switch, default file target setting, and autostart. It intentionally defers clipboard payload sync and file/folder transfer to follow-up plans because they are separate subsystems in the approved spec.
- Red-flag scan: No incomplete markers or unspecified code steps remain.
- Type consistency: Rust DTO names map to TypeScript DTO names through Serde snake_case output; `DeviceId` is a transparent Rust newtype and a TypeScript string; command names in `src/lib/api.ts` match the Rust `#[tauri::command]` names.
