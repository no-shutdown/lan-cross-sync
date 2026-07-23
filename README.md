# LAN Cross Sync（局域网跨设备同步）

LAN Cross Sync 是一个基于 Tauri v2、React、TypeScript 和 Rust 的桌面应用，用于在同一局域网内的 Windows 和 macOS 设备之间同步剪贴板内容和传输文件。

当前版本是可运行的最小可用版本（MVP）：已实现设备发现、6 位配对、配对设备授权连接、文本/图片剪贴板同步、文件和目录传输、断线重连、临时文件清理、系统托盘、开机启动，以及简体中文/英文界面切换。

项目定位是可信局域网内使用。目前未实现端到端加密、断点续传、带宽限制、剪贴板历史、互联网中继和 NAT 穿透。

网络端口采用 UDP/TCP 分离设计：UDP 发现和配对固定使用 `45731`，应用会同时使用全局和网卡定向广播；TCP 传输优先使用本地设置中的端口，如果被占用会自动选择空闲端口并广播实际端口。未配对设备超过 10 秒没有广播后会从列表移除，详细状态会显示在应用窗口顶部。

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
