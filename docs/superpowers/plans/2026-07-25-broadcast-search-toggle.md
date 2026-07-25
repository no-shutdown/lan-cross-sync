# 搜索设备开关 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增一个可持久化的"搜索设备"开关；关闭后在稳态下（所有已配对设备在线、无人在配对）停止对外广播，同时保证配对流程、断线重连、已连接设备的元数据同步（改名等）不受影响。

**Architecture:** `announce_loop`（`src-tauri/src/discovery.rs`）从无条件常驻广播，拆成两条独立的发送路径，复用在同一个 `DISCOVERY_INTERVAL` tick 里：①**广播路径**——按 `should_broadcast()` 门控条件（开关开启 / 正在配对 / 有已配对设备未连接）决定要不要向子网广播；②**心跳路径**——对 `connected_peer_endpoints()` 返回的每个已连接已配对设备，直接单播同一个 `Discovery` 包，不受开关影响，接收端复用现有 `apply_discovery_packet_at` 逻辑刷新对方的 IP 和 `DeviceInfo`（含改名）。不新增协议消息类型，不做 IP 磁盘持久化。

**Tech Stack:** Rust (Tauri backend, tokio async), TypeScript + React (前端，无自动化测试框架，仅 `tsc`/`vite build` 做类型检查)

## Global Constraints

- 新设置字段必须向后兼容：旧版本 `settings.json` 缺少该字段时，反序列化要默认为 `true`（保持现有"一直广播"的行为不变）。
- 不改动 `protocol.rs`、`transport.rs`；复用现有 `LanMessage::Discovery` 包格式与 `apply_discovery_packet_at` 接收逻辑。
- 不做已配对设备 IP 的磁盘持久化（应用重启后设备天然处于非 Connected 状态，会自动触发广播路径重新发现）。
- 每个 Rust 文件里新增/修改的 `LocalSettings` 结构体字面量都必须同步补上新字段，否则整个 crate 编译不过。

---

## Task 1: `LocalSettings.search_enabled` 字段与全部构造点修复

**Files:**
- Modify: `src-tauri/src/domain.rs:52-62`（`LocalSettings` 结构体 + 默认值函数）
- Modify: `src-tauri/src/domain.rs`（`mod tests` 内新增测试）
- Modify: `src-tauri/src/settings.rs:36-40`（默认设置构造）
- Modify: `src-tauri/src/commands.rs:449-453`（测试内字面量）
- Modify: `src-tauri/src/commands.rs:485-491`（`local_settings` 测试辅助函数）
- Modify: `src-tauri/src/clipboard.rs:425-437`（`settings_with_peer` 测试辅助函数）
- Modify: `src-tauri/src/clipboard.rs:441-446`（测试内字面量）
- Modify: `src-tauri/src/discovery.rs:372-377`（`test_pairing_runtime` 测试辅助函数）

**Interfaces:**
- Produces: `LocalSettings.search_enabled: bool` 字段；`domain::default_search_enabled() -> bool`（供 `#[serde(default = ...)]` 和其他文件构造默认值复用）。后续所有任务读取该字段时用 `settings.search_enabled`。

- [ ] **Step 1: 在 `domain.rs` 写失败的测试**

在 `src-tauri/src/domain.rs` 的 `mod tests` 里，紧跟在 `old_settings_get_default_locale_when_decoded` 测试后面加两个新测试：

```rust
    #[test]
    fn old_settings_get_default_search_enabled_when_decoded() {
        let raw = r#"{
            "local_device": {
                "id": "00000000-0000-0000-0000-000000000001",
                "name": "Windows Desk",
                "app_version": "0.1.0",
                "protocol_version": 1,
                "port": 45731,
                "capabilities": ["discovery"]
            },
            "paired_peers": []
        }"#;

        let settings: LocalSettings = serde_json::from_str(raw).unwrap();

        assert!(settings.search_enabled);
    }

    #[test]
    fn search_enabled_round_trips_through_serialization() {
        let settings = LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: Vec::new(),
            ui_locale: default_ui_locale(),
            search_enabled: false,
        };

        let json = serde_json::to_string(&settings).unwrap();
        let decoded: LocalSettings = serde_json::from_str(&json).unwrap();

        assert!(!decoded.search_enabled);
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib domain::tests -- --nocapture`
Expected: 编译失败，报错 `LocalSettings` 没有 `search_enabled` 字段（第二个测试的结构体字面量缺字段）。

- [ ] **Step 3: 在 `domain.rs` 实现字段与默认值函数**

把 `src-tauri/src/domain.rs:52-58` 的 `LocalSettings` 结构体：

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalSettings {
    pub local_device: DeviceInfo,
    pub paired_peers: Vec<PairedPeer>,
    #[serde(default = "default_ui_locale")]
    pub ui_locale: String,
}
```

改成：

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalSettings {
    pub local_device: DeviceInfo,
    pub paired_peers: Vec<PairedPeer>,
    #[serde(default = "default_ui_locale")]
    pub ui_locale: String,
    #[serde(default = "default_search_enabled")]
    pub search_enabled: bool,
}
```

紧接着 `default_ui_locale` 函数后面（`domain.rs:60-62` 之后）新增：

```rust
pub fn default_search_enabled() -> bool {
    true
}
```

- [ ] **Step 4: 修复其余 6 处结构体字面量，让 crate 能编译**

`src-tauri/src/settings.rs:36-40`，把：

```rust
        let settings = LocalSettings {
            local_device: DeviceInfo::new_local(device_name, DEFAULT_TRANSPORT_PORT),
            paired_peers: Vec::new(),
            ui_locale: default_ui_locale(),
        };
```

改成：

```rust
        let settings = LocalSettings {
            local_device: DeviceInfo::new_local(device_name, DEFAULT_TRANSPORT_PORT),
            paired_peers: Vec::new(),
            ui_locale: default_ui_locale(),
            search_enabled: default_search_enabled(),
        };
```

并把 `src-tauri/src/settings.rs:3` 的 import：

```rust
use crate::domain::{default_ui_locale, DeviceInfo, LocalSettings};
```

改成：

```rust
use crate::domain::{default_search_enabled, default_ui_locale, DeviceInfo, LocalSettings};
```

`src-tauri/src/commands.rs:449-453`，把：

```rust
        let settings = Arc::new(Mutex::new(LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: vec![peer.clone()],
            ui_locale: "zh-CN".to_string(),
        }));
```

改成：

```rust
        let settings = Arc::new(Mutex::new(LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: vec![peer.clone()],
            ui_locale: "zh-CN".to_string(),
            search_enabled: true,
        }));
```

`src-tauri/src/commands.rs:485-491`，把：

```rust
    fn local_settings(peers: Vec<PairedPeer>) -> LocalSettings {
        LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: peers,
            ui_locale: "zh-CN".to_string(),
        }
    }
```

改成：

```rust
    fn local_settings(peers: Vec<PairedPeer>) -> LocalSettings {
        LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: peers,
            ui_locale: "zh-CN".to_string(),
            search_enabled: true,
        }
    }
```

`src-tauri/src/clipboard.rs:425-437`，把：

```rust
    fn settings_with_peer(send_clipboard: bool) -> LocalSettings {
        LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: vec![PairedPeer {
                device: DeviceInfo::new_local("MacBook", 45731),
                receive_clipboard: true,
                send_clipboard,
                is_default_file_target: false,
                state: PeerConnectionState::Offline,
            }],
            ui_locale: "zh-CN".to_string(),
        }
    }
```

改成：

```rust
    fn settings_with_peer(send_clipboard: bool) -> LocalSettings {
        LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: vec![PairedPeer {
                device: DeviceInfo::new_local("MacBook", 45731),
                receive_clipboard: true,
                send_clipboard,
                is_default_file_target: false,
                state: PeerConnectionState::Offline,
            }],
            ui_locale: "zh-CN".to_string(),
            search_enabled: true,
        }
    }
```

`src-tauri/src/clipboard.rs:441-446`（`clipboard_polling_is_disabled_without_paired_devices` 测试里），把：

```rust
        let settings = LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: Vec::new(),
            ui_locale: "zh-CN".to_string(),
        };
```

改成：

```rust
        let settings = LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: Vec::new(),
            ui_locale: "zh-CN".to_string(),
            search_enabled: true,
        };
```

`src-tauri/src/discovery.rs:372-377`（`test_pairing_runtime` 里），把：

```rust
        let settings = LocalSettings {
            local_device: local_device.clone(),
            paired_peers: Vec::new(),
            ui_locale: "zh-CN".to_string(),
        };
```

改成：

```rust
        let settings = LocalSettings {
            local_device: local_device.clone(),
            paired_peers: Vec::new(),
            ui_locale: "zh-CN".to_string(),
            search_enabled: true,
        };
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -40`
Expected: 全部测试通过（PASS），无编译错误。

- [ ] **Step 6: 提交**

```bash
cd src-tauri
git add src/domain.rs src/settings.rs src/commands.rs src/clipboard.rs src/discovery.rs
git commit -m "feat: add search_enabled field to LocalSettings"
```

---

## Task 2: `set_search_enabled` 命令

**Files:**
- Modify: `src-tauri/src/commands.rs`（`set_ui_locale` 命令后新增，约在原 `commands.rs:253` 之后）
- Modify: `src-tauri/src/lib.rs:19`（导入列表）
- Modify: `src-tauri/src/lib.rs:279-295`（`invoke_handler` 注册列表）

**Interfaces:**
- Consumes: `LocalSettings.search_enabled`（Task 1 产出）
- Produces: `commands::set_search_enabled(state, enabled: bool) -> AppResult<LocalSettings>`，前端通过 Tauri `invoke('set_search_enabled', { enabled })` 调用。

此命令逻辑是纯字段赋值 + 落盘，和现有 `set_ui_locale`（`commands.rs:242-253`）同一模式；该模式在本代码库里一贯不写命令层单测（`set_ui_locale`、`set_device_name` 都没有），命令层的正确性由 Task 1 的 `search_enabled` 序列化测试和后面的手工验证共同覆盖。这里不额外造一个不符合仓库习惯的测试。

- [ ] **Step 1: 实现命令**

在 `src-tauri/src/commands.rs` 里，紧跟在 `set_ui_locale` 函数（`commands.rs:242-253`）后面插入：

```rust
#[tauri::command]
pub fn set_search_enabled(state: State<'_, AppState>, enabled: bool) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    let mut next = settings.clone();
    next.search_enabled = enabled;
    state.settings_store.save(&next)?;
    *settings = next.clone();
    Ok(next)
}
```

- [ ] **Step 2: 在 `lib.rs` 注册命令**

`src-tauri/src/lib.rs:16-21` 的 import 列表：

```rust
use commands::{
    accept_file_transfer, cancel_file_transfer, cancel_pairing, clear_pairing,
    get_autostart_enabled, get_dashboard_state, request_pairing, set_autostart_enabled,
    set_default_file_target, set_device_name, set_receive_clipboard, set_send_clipboard,
    set_ui_locale, start_file_transfer, start_pairing, AppState, NetworkStatus,
};
```

改成（新增 `set_search_enabled`，按字母序插入 `set_receive_clipboard` 之前）：

```rust
use commands::{
    accept_file_transfer, cancel_file_transfer, cancel_pairing, clear_pairing,
    get_autostart_enabled, get_dashboard_state, request_pairing, set_autostart_enabled,
    set_default_file_target, set_device_name, set_receive_clipboard, set_search_enabled,
    set_send_clipboard, set_ui_locale, start_file_transfer, start_pairing, AppState,
    NetworkStatus,
};
```

`src-tauri/src/lib.rs:279-295` 的 `invoke_handler` 列表：

```rust
        .invoke_handler(tauri::generate_handler![
            get_dashboard_state,
            start_file_transfer,
            accept_file_transfer,
            cancel_file_transfer,
            get_autostart_enabled,
            set_autostart_enabled,
            start_pairing,
            cancel_pairing,
            request_pairing,
            set_receive_clipboard,
            set_send_clipboard,
            set_default_file_target,
            set_device_name,
            set_ui_locale,
            clear_pairing
        ])
```

改成（新增一行 `set_search_enabled,`）：

```rust
        .invoke_handler(tauri::generate_handler![
            get_dashboard_state,
            start_file_transfer,
            accept_file_transfer,
            cancel_file_transfer,
            get_autostart_enabled,
            set_autostart_enabled,
            start_pairing,
            cancel_pairing,
            request_pairing,
            set_receive_clipboard,
            set_send_clipboard,
            set_default_file_target,
            set_device_name,
            set_ui_locale,
            set_search_enabled,
            clear_pairing
        ])
```

- [ ] **Step 3: 编译确认**

Run: `cd src-tauri && cargo check`
Expected: 编译成功，无错误。

- [ ] **Step 4: 提交**

```bash
cd src-tauri
git add src/commands.rs src/lib.rs
git commit -m "feat: add set_search_enabled command"
```

---

## Task 3: `should_broadcast` 门控函数

**Files:**
- Modify: `src-tauri/src/discovery.rs:1-16`（imports）
- Modify: `src-tauri/src/discovery.rs`（新函数，放在 `apply_discovery_packet_at` 之后、`announce_loop` 之前，约第 100 行前）
- Test: `src-tauri/src/discovery.rs`（`mod tests` 内新增）

**Interfaces:**
- Consumes: `PairedPeer`、`PeerConnectionState`（`domain.rs` 既有类型）
- Produces: `pub fn should_broadcast(search_enabled: bool, pairing_active: bool, paired_peers: &[PairedPeer]) -> bool`，Task 5 会在 `announce_loop` 里调用。

- [ ] **Step 1: 写失败的测试**

在 `src-tauri/src/discovery.rs` 的 `mod tests` 里（`use super::*;` 之后，任意位置，建议紧跟在 `broadcast_address_uses_selected_port` 测试后面）新增：

```rust
    fn paired_peer_with_state(state: PeerConnectionState) -> PairedPeer {
        PairedPeer {
            device: DeviceInfo::new_local("MacBook", 45731),
            receive_clipboard: true,
            send_clipboard: true,
            is_default_file_target: false,
            state,
        }
    }

    #[test]
    fn should_broadcast_when_search_enabled() {
        assert!(should_broadcast(true, false, &[]));
    }

    #[test]
    fn should_broadcast_when_pairing_active() {
        assert!(should_broadcast(false, true, &[]));
    }

    #[test]
    fn should_broadcast_when_a_paired_peer_is_not_connected() {
        let peers = vec![
            paired_peer_with_state(PeerConnectionState::Connected),
            paired_peer_with_state(PeerConnectionState::Offline),
        ];
        assert!(should_broadcast(false, false, &peers));
    }

    #[test]
    fn should_not_broadcast_in_steady_state() {
        let peers = vec![paired_peer_with_state(PeerConnectionState::Connected)];
        assert!(!should_broadcast(false, false, &peers));
    }

    #[test]
    fn should_not_broadcast_with_no_paired_peers_and_switch_off() {
        assert!(!should_broadcast(false, false, &[]));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib discovery::tests -- --nocapture`
Expected: 编译失败，报错找不到 `should_broadcast` 函数。

- [ ] **Step 3: 实现 `should_broadcast`**

把 `src-tauri/src/discovery.rs:1-9` 的 imports：

```rust
use crate::{
    domain::{DeviceId, DeviceInfo, LocalSettings},
    pairing::PairingRuntime,
    protocol::{
        decode_message, encode_message, DiscoveryPacket, LanMessage, PairingConfirm,
        PairingRequest, PairingResponse, PROTOCOL_VERSION,
    },
    registry::PeerRegistry,
};
```

改成：

```rust
use crate::{
    domain::{DeviceId, DeviceInfo, LocalSettings, PairedPeer, PeerConnectionState},
    pairing::{PairingRuntime, PairingSession},
    protocol::{
        decode_message, encode_message, DiscoveryPacket, LanMessage, PairingConfirm,
        PairingRequest, PairingResponse, PROTOCOL_VERSION,
    },
    registry::PeerRegistry,
};
```

在 `apply_discovery_packet_at` 函数（`discovery.rs:82-98`）后面、`announce_loop` 函数前面插入：

```rust
/// Whether this device should broadcast a discovery packet to the whole
/// subnet on this tick. Steady-state (search switch off, nobody actively
/// pairing, every paired peer already connected) is the only case that
/// returns false — everything else needs the wider reach a broadcast gives
/// to be found by, or to re-find, a peer.
pub fn should_broadcast(
    search_enabled: bool,
    pairing_active: bool,
    paired_peers: &[PairedPeer],
) -> bool {
    search_enabled
        || pairing_active
        || paired_peers
            .iter()
            .any(|peer| peer.state != PeerConnectionState::Connected)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test --lib discovery::tests -- --nocapture`
Expected: 新增的 5 个测试全部 PASS（其余 discovery 测试也应保持 PASS）。

- [ ] **Step 5: 提交**

```bash
cd src-tauri
git add src/discovery.rs
git commit -m "feat: add should_broadcast gating function"
```

---

## Task 4: `connected_peer_endpoints` 心跳目标函数

**Files:**
- Modify: `src-tauri/src/discovery.rs`（新函数，紧跟在 Task 3 的 `should_broadcast` 后面）
- Test: `src-tauri/src/discovery.rs`（`mod tests` 内新增）

**Interfaces:**
- Consumes: `PeerRegistry::paired()`、`PeerRegistry::discovery_endpoint(&DeviceId)`（`registry.rs` 既有公开方法，未修改）
- Produces: `pub fn connected_peer_endpoints(registry: &PeerRegistry) -> Vec<SocketAddr>`，Task 5 会在 `announce_loop` 里调用，把结果作为单播心跳的目标地址。

- [ ] **Step 1: 写失败的测试**

在 `src-tauri/src/discovery.rs` 的 `mod tests` 里，紧跟在 Task 3 新增的测试后面新增：

```rust
    #[test]
    fn connected_peer_endpoints_includes_only_connected_devices_with_known_address() {
        let mut registry = PeerRegistry::new();
        let connected = DeviceInfo::new_local("MacBook", 45731);
        let offline = DeviceInfo::new_local("Linux Desk", 45731);
        registry.set_paired(PairedPeer {
            device: connected.clone(),
            receive_clipboard: true,
            send_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        });
        registry.set_paired(PairedPeer {
            device: offline,
            receive_clipboard: true,
            send_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Offline,
        });
        let source: SocketAddr = "192.0.2.20:54321".parse().unwrap();
        registry.mark_discovered_at(connected.clone(), source);

        let endpoints = connected_peer_endpoints(&registry);

        assert_eq!(endpoints, vec!["192.0.2.20:45731".parse().unwrap()]);
    }

    #[test]
    fn connected_peer_endpoints_skips_connected_device_with_no_known_address() {
        let mut registry = PeerRegistry::new();
        registry.set_paired(PairedPeer {
            device: DeviceInfo::new_local("MacBook", 45731),
            receive_clipboard: true,
            send_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        });

        assert!(connected_peer_endpoints(&registry).is_empty());
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib discovery::tests -- --nocapture`
Expected: 编译失败，报错找不到 `connected_peer_endpoints` 函数。

- [ ] **Step 3: 实现 `connected_peer_endpoints`**

紧跟在 Task 3 加的 `should_broadcast` 函数后面插入：

```rust
/// Unicast heartbeat targets: every paired peer we currently believe is
/// connected, paired with the discovery-socket address we last learned for
/// it. Sending a plain `Discovery` packet directly to each of these keeps
/// their copy of our `DeviceInfo` (name, etc.) fresh without ever touching
/// the subnet broadcast address, so it works even while the search switch
/// is off.
pub fn connected_peer_endpoints(registry: &PeerRegistry) -> Vec<SocketAddr> {
    registry
        .paired()
        .into_iter()
        .filter(|peer| peer.state == PeerConnectionState::Connected)
        .filter_map(|peer| registry.discovery_endpoint(&peer.device.id))
        .collect()
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test --lib discovery::tests -- --nocapture`
Expected: 新增 2 个测试 PASS，其余 discovery 测试保持 PASS。

- [ ] **Step 5: 提交**

```bash
cd src-tauri
git add src/discovery.rs
git commit -m "feat: add connected_peer_endpoints heartbeat target function"
```

---

## Task 5: 重写 `announce_loop`，接入门控与心跳

**Files:**
- Modify: `src-tauri/src/discovery.rs:100-130`（`announce_loop` 函数体与签名）
- Modify: `src-tauri/src/lib.rs:145-154`（调用点）

**Interfaces:**
- Consumes: `should_broadcast`（Task 3）、`connected_peer_endpoints`（Task 4）、`AppState`/`app.manage` 里已有的 `registry: Arc<Mutex<PeerRegistry>>` 与 `active_pairing: Arc<Mutex<Option<PairingSession>>>`（`lib.rs` 既有变量，未修改类型）
- Produces: `announce_loop(settings, registry, active_pairing, port)` 新签名——多了 `registry` 和 `active_pairing` 两个参数，供 `lib.rs` 调用点传入。

这个任务改的是异步网络循环本身，不适合再拆出可脱离网络单测的纯逻辑（门控和心跳目标已经在 Task 3/4 里各自测过了），所以这里只做编译级验证 + 全量测试回归，不新增测试。

- [ ] **Step 1: 重写 `announce_loop` 函数体**

把 `src-tauri/src/discovery.rs:100-130`：

```rust
pub async fn announce_loop(settings: Arc<Mutex<LocalSettings>>, port: u16) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0))
        .await
        .context("failed to bind discovery UDP socket")?;
    socket
        .set_broadcast(true)
        .context("failed to enable discovery UDP broadcast")?;
    let targets = match interface_broadcasts() {
        Ok(addresses) => discovery_targets(port, addresses),
        Err(err) => {
            tracing::warn!(?err, "using global discovery broadcast only");
            vec![discovery_socket_addr(port)]
        }
    };
    let mut interval = time::interval(DISCOVERY_INTERVAL);

    loop {
        interval.tick().await;
        // Re-read the local device on every tick (instead of once at
        // startup) so a rename via `set_device_name` is picked up by the
        // very next broadcast rather than only after an app restart.
        let device = settings.lock().unwrap().local_device.clone();
        let payload = encode_discovery(device)?;
        for target in &targets {
            socket
                .send_to(&payload, target)
                .await
                .with_context(|| format!("failed to send discovery packet to {target}"))?;
        }
    }
}
```

改成：

```rust
pub async fn announce_loop(
    settings: Arc<Mutex<LocalSettings>>,
    registry: Arc<Mutex<PeerRegistry>>,
    active_pairing: Arc<Mutex<Option<PairingSession>>>,
    port: u16,
) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0))
        .await
        .context("failed to bind discovery UDP socket")?;
    socket
        .set_broadcast(true)
        .context("failed to enable discovery UDP broadcast")?;
    let targets = match interface_broadcasts() {
        Ok(addresses) => discovery_targets(port, addresses),
        Err(err) => {
            tracing::warn!(?err, "using global discovery broadcast only");
            vec![discovery_socket_addr(port)]
        }
    };
    let mut interval = time::interval(DISCOVERY_INTERVAL);

    loop {
        interval.tick().await;
        // Re-read the local device and switch state on every tick (instead
        // of once at startup) so a rename via `set_device_name`, or
        // flipping the search switch, is picked up by the very next tick
        // rather than only after an app restart.
        let (device, search_enabled) = {
            let settings = settings.lock().unwrap();
            (settings.local_device.clone(), settings.search_enabled)
        };
        let payload = encode_discovery(device)?;
        let pairing_active = active_pairing.lock().unwrap().is_some();
        let (paired_peers, heartbeat_targets) = {
            let registry = registry.lock().unwrap();
            (registry.paired(), connected_peer_endpoints(&registry))
        };

        if should_broadcast(search_enabled, pairing_active, &paired_peers) {
            for target in &targets {
                socket
                    .send_to(&payload, target)
                    .await
                    .with_context(|| format!("failed to send discovery packet to {target}"))?;
            }
        }

        for endpoint in heartbeat_targets {
            socket
                .send_to(&payload, endpoint)
                .await
                .with_context(|| format!("failed to send discovery heartbeat to {endpoint}"))?;
        }
    }
}
```

- [ ] **Step 2: 更新 `lib.rs` 调用点**

把 `src-tauri/src/lib.rs:145-154`：

```rust
            if advertising {
                let announce_settings = settings.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) =
                        discovery::announce_loop(announce_settings, DEFAULT_DISCOVERY_PORT).await
                    {
                        tracing::error!(?err, "LAN discovery announcer stopped");
                    }
                });
            }
```

改成：

```rust
            if advertising {
                let announce_settings = settings.clone();
                let announce_registry = registry.clone();
                let announce_active_pairing = active_pairing.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = discovery::announce_loop(
                        announce_settings,
                        announce_registry,
                        announce_active_pairing,
                        DEFAULT_DISCOVERY_PORT,
                    )
                    .await
                    {
                        tracing::error!(?err, "LAN discovery announcer stopped");
                    }
                });
            }
```

（`registry` 和 `active_pairing` 在这个作用域里已经存在——分别在 `lib.rs:106` 和 `lib.rs:107` 定义，本步骤只是多 `clone()` 两份 `Arc` 传进去，不需要新建。）

- [ ] **Step 3: 编译并跑全量测试**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -60`
Expected: 全部测试 PASS，包括 Task 1/3/4 新增的测试和原有的 `discovery.rs`/`transport.rs`/`commands.rs` 测试。

- [ ] **Step 4: 提交**

```bash
cd src-tauri
git add src/discovery.rs src/lib.rs
git commit -m "feat: split announce_loop into gated broadcast and unicast heartbeat paths"
```

---

## Task 6: 前端类型与 API

**Files:**
- Modify: `src/lib/types.ts:22-26`（`LocalSettings` 接口）
- Modify: `src/lib/api.ts`（新增 `setSearchEnabled`，紧跟在 `setUiLocale` 后面）

**Interfaces:**
- Consumes: 后端 `set_search_enabled` 命令（Task 2）、`DashboardState.settings.search_enabled`（Task 1 字段会随 `get_dashboard_state` 一起序列化下发，`DashboardState` 接口本身不用改，因为它内嵌的就是 `LocalSettings`）
- Produces: `setSearchEnabled(enabled: boolean): Promise<LocalSettings>`，Task 8 的 UI 会调用它。

前端没有测试框架，这个任务用 `tsc` 类型检查代替单测。

- [ ] **Step 1: 更新 `LocalSettings` 类型**

把 `src/lib/types.ts:22-26`：

```typescript
export interface LocalSettings {
  local_device: DeviceInfo
  paired_peers: PairedPeer[]
  ui_locale: Locale
}
```

改成：

```typescript
export interface LocalSettings {
  local_device: DeviceInfo
  paired_peers: PairedPeer[]
  ui_locale: Locale
  search_enabled: boolean
}
```

- [ ] **Step 2: 新增 `setSearchEnabled` API 函数**

在 `src/lib/api.ts` 里，紧跟在 `setUiLocale`（`api.ts:28-30`）后面插入：

```typescript
export function setSearchEnabled(enabled: boolean): Promise<LocalSettings> {
  return invoke('set_search_enabled', { enabled })
}
```

- [ ] **Step 3: 类型检查**

Run: `pnpm exec tsc --noEmit`
Expected: 无类型错误。

- [ ] **Step 4: 提交**

```bash
git add src/lib/types.ts src/lib/api.ts
git commit -m "feat: add search_enabled type and setSearchEnabled API call"
```

---

## Task 7: 前端文案

**Files:**
- Modify: `src/lib/i18n.ts:25-26`（zh-CN，`startup`/`autostart` 之间）
- Modify: `src/lib/i18n.ts:102-103`（en-US，对应位置）

**Interfaces:**
- Produces: `MessageKey` 新增 `searchDevices` / `searchDevicesHint`，Task 8 的 UI 用 `t(locale, 'searchDevices')` 等读取。

- [ ] **Step 1: 新增 zh-CN 文案**

把 `src/lib/i18n.ts:25-26`：

```typescript
    startup: '启动设置',
    autostart: '系统启动时运行',
```

改成：

```typescript
    startup: '启动设置',
    autostart: '系统启动时运行',
    searchDevices: '搜索设备',
    searchDevicesHint: '关闭后，仅在有设备离线待重连或正在配对时临时广播',
```

- [ ] **Step 2: 新增 en-US 文案**

把 `src/lib/i18n.ts:102-103`：

```typescript
    startup: 'Startup',
    autostart: 'Run when the system starts',
```

改成：

```typescript
    startup: 'Startup',
    autostart: 'Run when the system starts',
    searchDevices: 'Search for devices',
    searchDevicesHint: 'When off, only broadcasts while reconnecting an offline paired device or actively pairing',
```

- [ ] **Step 3: 类型检查**

Run: `pnpm exec tsc --noEmit`
Expected: 无类型错误（`MessageKey` 是 `keyof typeof messages['zh-CN']` 自动推导，新增键后两个 locale 对象结构必须一致，否则 `en-US` 对象会报缺字段/多字段错误）。

- [ ] **Step 4: 提交**

```bash
git add src/lib/i18n.ts
git commit -m "feat: add search-devices toggle copy (zh-CN, en-US)"
```

---

## Task 8: 前端 UI 开关

**Files:**
- Modify: `src/App.tsx:1539-1550`（设置面板 section）
- Modify: `src/App.tsx:1323-1330`（在 `updateLocale` 附近新增 `toggleSearchEnabled` 函数）

**Interfaces:**
- Consumes: `setSearchEnabled`（Task 6）、`t(locale, 'searchDevices' | 'searchDevicesHint')`（Task 7）、`dashboard.settings.search_enabled`（Task 1，随 `getDashboardState()` 下发）

- [ ] **Step 1: 新增切换函数**

在 `src/App.tsx` 里，紧跟在 `updateLocale` 函数（`App.tsx:1323-1330`）后面插入：

```typescript
  async function toggleSearchEnabled() {
    if (!dashboard) return
    try {
      await setSearchEnabled(!dashboard.settings.search_enabled)
      await refresh()
    } catch (err) {
      setError(backendError(locale, err, t(locale, 'errorTransfer')))
    }
  }
```

并在文件顶部的 import 列表里加入 `setSearchEnabled`（跟 `setDeviceName`、`setUiLocale` 等放在一起，按现有 import 顺序插入即可）。

- [ ] **Step 2: 在设置面板加开关**

把 `src/App.tsx:1539-1550`：

```tsx
      <section className="panel settings-panel">
        <h2>{t(locale, 'startup')}</h2>
        <label className="check-row">
          <input
            type="checkbox"
            checked={Boolean(autostart)}
            disabled={autostart === null || autostartPending}
            onChange={() => void toggleAutostart()}
          />
          {t(locale, 'autostart')}
        </label>
      </section>
    </main>
  )
}
```

改成：

```tsx
      <section className="panel settings-panel">
        <h2>{t(locale, 'startup')}</h2>
        <label className="check-row">
          <input
            type="checkbox"
            checked={Boolean(autostart)}
            disabled={autostart === null || autostartPending}
            onChange={() => void toggleAutostart()}
          />
          {t(locale, 'autostart')}
        </label>
        <label className="check-row">
          <input
            type="checkbox"
            checked={dashboard.settings.search_enabled}
            onChange={() => void toggleSearchEnabled()}
          />
          {t(locale, 'searchDevices')}
        </label>
        <p className="hint">{t(locale, 'searchDevicesHint')}</p>
      </section>
    </main>
  )
}
```

- [ ] **Step 3: 类型检查与构建**

Run: `pnpm exec tsc --noEmit && pnpm run build`
Expected: 无类型错误，构建成功。

- [ ] **Step 4: 提交**

```bash
git add src/App.tsx
git commit -m "feat: add search-devices toggle to settings panel"
```

---

## Task 9: 整体验证

**Files:** 无新增/修改，纯验证步骤。

- [ ] **Step 1: 后端全量测试**

Run: `cd src-tauri && cargo test 2>&1 | tail -80`
Expected: 全部测试 PASS（含所有既有测试 + Task 1/3/4 新增测试），无警告级别的新增 `dead_code`/`unused` 报错。

- [ ] **Step 2: 后端 lint**

Run: `cd src-tauri && cargo clippy --all-targets -- -D warnings`
Expected: 无 clippy 报错。

- [ ] **Step 3: 前端构建**

Run: `pnpm run build`
Expected: 构建成功（内部先跑 `tsc` 再跑 `vite build`）。

- [ ] **Step 4: 手工验证（真实跑一次应用）**

Run: `pnpm tauri dev`

验证清单：
1. 打开设置面板，确认能看到"搜索设备"开关，默认是勾选状态（对应 `search_enabled` 默认 `true`）。
2. 关闭开关，确认设置能持久化（重启 `pnpm tauri dev` 后开关仍是关闭状态）。
3. 在两台设备（或两个本机实例，分别指向不同 `settings.json`）之间，关闭其中一台的"搜索设备"，确认稳态下点不到新设备了；点击"生成配对码"后，另一台仍能发现并完成配对。
4. 两台设备配对且都在线时，在其中一台改名，确认另一台的"已配对设备"列表几秒内显示新名字——即使把改名那台的"搜索设备"开关关掉也应该照常同步。

- [ ] **Step 5: 提交（如手工验证中有微调）**

如果 Step 4 发现需要修的小问题，修完后：

```bash
git add -A
git commit -m "fix: address manual verification findings for search-devices toggle"
```

如果没有问题，本任务不产生新提交。

---

## Self-Review 记录

- **Spec 覆盖**：设计文档（`docs/superpowers/specs/2026-07-25-broadcast-search-toggle-design.md`）里的 4 点方案——① `search_enabled` 设置项 ② `announce_loop` 拆两条路径 ③ 不做 IP 落盘 ④ 不新增协议消息——分别对应 Task 1/2、Task 3/4/5、Task 5 的实现说明、Task 3/4/5 未涉及 `protocol.rs`/`transport.rs`。触发条件表格的四行场景在 Task 9 Step 4 的手工验证清单里逐条覆盖。
- **占位符检查**：全文没有 TBD/TODO，每个代码步骤都是可直接套用的完整代码块。
- **类型一致性**：`should_broadcast(search_enabled: bool, pairing_active: bool, paired_peers: &[PairedPeer]) -> bool` 和 `connected_peer_endpoints(registry: &PeerRegistry) -> Vec<SocketAddr>` 的签名在 Task 3/4（定义处）和 Task 5（调用处）保持一致；`announce_loop` 新签名在 Task 5 Step 1（定义）和 Step 2（调用）保持一致；前端 `setSearchEnabled`/`search_enabled` 命名在 Task 6/7/8 之间保持一致。
