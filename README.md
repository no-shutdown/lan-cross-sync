# LAN Cross Sync（局域网跨设备同步）

LAN Cross Sync 是一个基于 Tauri v2、React、TypeScript 和 Rust 的桌面应用，用于在同一局域网内的 Windows 和 macOS 设备之间同步剪贴板内容和传输文件。

当前版本是可运行的最小可用版本（MVP）：已实现可独立开关的设备发现、6 位配对、配对设备授权连接、文本/图片剪贴板同步、文件和目录传输、断线重连、临时文件清理、系统托盘、开机启动，以及简体中文/英文界面切换。

项目定位是可信局域网内使用。目前未实现端到端加密、断点续传、带宽限制、剪贴板历史、互联网中继和 NAT 穿透。

## 设备发现与连接

网络端口采用 UDP/TCP 分离设计：UDP 发现和配对固定使用 `45731`（全局广播 + 各网卡定向广播）；TCP 传输优先使用本地设置中的端口，如果被占用会自动选择空闲端口并广播实际端口。启动时先绑定 UDP 和 TCP 端点，只有两者都可用才可能广播本机，绑定失败会在网络状态区域明确显示。

应用窗口的"网络状态"区域提供两个独立开关（默认都开启），并实时显示当前是否真的在广播：

- **可被发现**：控制本机是否向子网广播自己的信息（发送侧）。关闭后就真的不再广播，没有例外。生成配对码依赖"能被对方扫到"，所以要求"可被发现"必须先开启，未开启时"生成配对码"按钮直接禁用；配对等待过程中把"可被发现"关掉，会自动取消这次配对。
- **搜索设备**：控制是否把收到的、尚未配对的设备加入"已发现设备"列表（接收侧）。关闭后不再显示新设备，并会立即清空当前列表；已配对设备的重连和信息同步不受影响。

设备当前 IP 完全不落盘持久化，只存在于内存里，且只能靠"收到对方的包"来更新——这决定了两件事：

- **已连接设备之间的信息同步**：只要两台设备处于已连接状态，无论"可被发现"开关状态如何，每 3 秒都会互相单播一次心跳包（发到内存里记录的对方地址），同步最新的设备信息（比如改名）并维持连接存活。
- **掉线重连**：如果某台已配对设备的 IP 变了（换网络、重启等），连接会断开，且不会因为"对方掉线"这件事自动恢复广播（这条覆盖逻辑已移除）。要重新连上，需要 **IP 变了的那一方** 把"可被发现"打开，对方才能收到新的广播、刷新内存里记录的地址，再据此发起 TCP 重连——单纯另一方开着"可被发现"广播自己没有用，因为对方缺的是"新地址"而不是"我的地址"。如果两台设备同时关闭"可被发现"、且恰好这时候一方 IP 变了，会连不上，需要手动临时打开开关。

未配对设备连续 10 秒没有收到广播，会从"已发现设备"列表移除；已配对设备记录不会因此被删除，只是连接状态会变化。

## 快速开始

安装依赖：

```powershell
pnpm install --frozen-lockfile
```

启动开发调试前，请先自行安装 Rust stable MSVC 工具链，并确保 `cargo` 已加入系统 PATH。验证开发环境：

```powershell
cargo --version
pnpm tauri dev
```

如果 `cargo --version` 找不到命令，请由开发者配置 Rust 的系统环境变量并重新打开终端；项目不会自动修改 PATH。完整的环境准备、双机验收、故障排查和打包流程见：

- [`docs/PROJECT_GUIDE.md`](docs/PROJECT_GUIDE.md)：当前功能、架构、限制和数据边界。
- [`docs/BUILD_AND_TEST.md`](docs/BUILD_AND_TEST.md)：开发运行、自动化验证、Windows 安装包和 macOS DMG。
- [`docs/superpowers/`](docs/superpowers/)：历史设计与实施记录，不代表当前待办清单。

## 常用验证命令

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --all -- --check
cargo check --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml
pnpm build
```

## Windows 打包

```powershell
pnpm tauri build --no-sign
```

NSIS、MSI 和裸发布版可执行文件会生成在 `src-tauri\target\release\` 下。普通用户优先使用 `bundle\nsis\*-setup.exe`；MSI 更适合企业部署。macOS 安装包必须在 macOS 或 macOS 持续集成环境上构建，Windows 不能直接生成 DMG。

Windows 本地设置通常位于以下系统路径。这里使用的是系统环境变量，不包含具体用户名：

```text
%APPDATA%\com.local.lancrosssync\settings.json
```
