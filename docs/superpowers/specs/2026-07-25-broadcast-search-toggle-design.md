# 搜索设备开关 设计文档

**日期**: 2026-07-25
**状态**: 已批准

---

## 背景

当前 `announce_loop`（`src-tauri/src/discovery.rs`）是一个无条件常驻循环：不管有没有已配对设备、有没有人在找新设备，每 3 秒都会向整个子网广播一次本机 `DeviceInfo`。

实际使用中，一旦设备之间已经配对且保持连接，后续绝大多数时间根本不需要广播——广播只在两种场景下才真正有用：

1. 用户主动想找新设备（配对流程）
2. 某个已配对设备掉线了，需要重新发现它当前的 IP 才能重连

而"已配对且已连接"的设备之间，目前也是靠广播包顺带更新对方的名字等元数据（`registry.rs` 的 `upsert_discovered` 对已配对设备会无条件覆盖 `peer.device`）。如果把广播关掉，这条元数据更新的路子也会跟着断。

## 目标

- 新增一个用户可控的"搜索设备"开关，关闭后在稳态下（所有已配对设备都在线、没人在配对）不再对外广播。
- 开关关闭不能影响：①正在进行的配对流程 ②已配对设备掉线后的自动重连 ③已连接设备之间的元数据（如改名）同步。
- 最小改动：不碰通信协议、不新增持久化字段，复用现有的 `LanMessage::Discovery` 包和接收端处理逻辑。

---

## 设计方案

### 1. 新设置项 `search_enabled`

`LocalSettings`（`domain.rs`）新增字段：

```rust
#[serde(default = "default_search_enabled")]
pub search_enabled: bool,
```

默认 `true`（旧配置文件缺该字段时按开启处理，保持现有行为不变）。

新命令 `set_search_enabled(enabled: bool) -> LocalSettings`，实现方式照抄 `set_receive_clipboard`（`commands.rs:256`）：落盘 + 更新内存 `settings`。

前端设置面板新增一个开关，文案"搜索设备"，与语言选择、开机自启同级。

### 2. `announce_loop` 拆成两条发送路径

`announce_loop` 签名从 `(settings, port)` 扩展为 `(settings, registry, active_pairing, port)`，同一个 `DISCOVERY_INTERVAL`（3 秒）tick 里依次跑两条路径：

**广播路径**（发到 `discovery_targets`，即子网广播地址，包内容和现在完全一样）：

```
发广播 = settings.search_enabled
      OR active_pairing 非空       // 本机正在生成配对码，等待被扫到
      OR 存在任意已配对设备状态 != Connected   // 掉线待重连
```

- 开关开着：和现在行为完全一致，无条件每 3 秒广播一次。
- 开关关着：默认不发；只有"有已配对设备没连上"或"正在配对"时才发，一旦条件消失（重连成功 / 配对结束）自动停止。

**心跳路径**（新增，不受开关影响，始终执行）：

对 `registry.paired()` 中状态为 `Connected` 的每个设备，取其 `registry.discovery_endpoint(id)`（连接期间已经记录在内存里的对方 UDP 地址），直接**单播**发送同一个 `encode_discovery(local_device)` 包过去，不经过广播地址。

接收端零改动：`handle_lan_message` → `apply_discovery_packet_at` → `decode_discovery` 这条链路本来就不区分包是广播来的还是单播来的，收到后照常刷新发送方的 `discovery_endpoint`/`transport_endpoint` 和 `DeviceInfo`（包括名字）。由于双方都会在自己的 tick 里给对方发一份，天然形成双向心跳：A 发的包刷新 B 的 registry，B 发的包刷新 A 的 registry。

### 3. 为什么不需要额外持久化 IP

应用重启后 `PeerRegistry::from_paired` 重建出的已配对设备状态默认就是配置文件里存的最后状态（通常不是 `Connected`，因为 `PairedPeer` 落盘时机不会记录"进程退出瞬间"的实时连接态，且新进程里 `transport` 尚未建立任何连接）。这天然满足"广播路径"里 `状态 != Connected` 的条件，会自动触发广播重新发现，不需要把 IP 落盘兜底。

### 4. 为什么不新增协议消息

已连接设备的元数据同步直接复用现有 `LanMessage::Discovery` 包，通过单播发送即可，不需要新增 `TransportMessage` 变体或改动 `transport.rs`。

---

## 触发条件一览

| 场景 | search_enabled=true | search_enabled=false |
|------|---------------------|----------------------|
| 稳态（全部已配对设备 Connected，无人配对） | 广播（不变） | 不广播，仅心跳路径单播 |
| 有已配对设备掉线 | 广播（不变） | 自动恢复广播，直到重连或被移除 |
| 本机点击"开始配对" | 广播（不变） | 自动临时广播，直到配对完成/取消 |
| 已连接设备改名 | 广播 + 心跳单播都会更新对方 | 仅心跳单播更新对方 |

---

## 文件变更清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `src-tauri/src/domain.rs` | 修改 | `LocalSettings` 新增 `search_enabled` 字段及默认值函数 |
| `src-tauri/src/discovery.rs` | 修改 | `announce_loop` 拆分广播路径 + 心跳路径，签名新增 `registry`、`active_pairing` 参数 |
| `src-tauri/src/commands.rs` | 修改 | 新增 `set_search_enabled` 命令 |
| `src-tauri/src/lib.rs` | 修改 | `announce_loop` 调用点传入新增参数 |
| `src/lib/api.ts` / `src/lib/types.ts` | 修改 | 新增 `setSearchEnabled` 调用与 `search_enabled` 类型字段 |
| `src/App.tsx` | 修改 | 设置面板新增开关 UI |
| `src/lib/i18n.ts` | 修改 | 新增开关文案的中英文词条 |

不需要改动 `protocol.rs`、`transport.rs`、`registry.rs`（复用现有 `discovery_endpoint` 查询方法）。

---

## 边界情况

| 场景 | 处理方式 |
|------|---------|
| 已配对设备状态为 `Discovered`/`Pairing`/`Error`（非 Connected） | 按"未连接"处理，计入广播路径触发条件 |
| 已连接设备的 `discovery_endpoint` 因某种原因在 registry 中缺失 | 心跳路径跳过该设备（无地址可发），不报错；下次该设备状态变化或触发广播路径时会自然补上 |
| 心跳单播包在网络层丢失 | 无重试，等下一个 3 秒 tick 自然重发，容忍偶发丢包 |
| 用户在配对过程中关闭开关 | `active_pairing` 条件仍然成立，不影响正在进行的配对 |

---

## 测试策略

- `discovery.rs`：新增单测覆盖广播路径的三种触发条件组合（开关开、开关关+有未连接设备、开关关+配对中、开关关+稳态不发）。
- `discovery.rs`：心跳路径单测——已连接设备通过单播收到 discovery 包后，接收端 registry 里的 `device`（名字）和端点被正确刷新。
- `commands.rs`：`set_search_enabled` 落盘与读取的 round-trip 测试。

---

## 不在此次范围内

- 广播/心跳发送状态的前端实时展示（比如"正在广播中"提示）
- 心跳路径使用独立于 `DISCOVERY_INTERVAL` 的自定义频率
- 已配对设备 IP 的磁盘持久化
