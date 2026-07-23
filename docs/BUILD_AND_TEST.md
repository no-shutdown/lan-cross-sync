# 运行、验证与打包

本文档描述当前仓库的开发运行、双机验收和发布包生成流程。安装包用户不需要安装源码、Node.js 或 Rust；这些依赖只用于开发和构建。

## 1. 开发环境

### Windows

需要安装：

- Node.js LTS 和 pnpm。
- Rust stable MSVC 工具链。
- Visual Studio Build Tools 的 `Desktop development with C++` 工作负载。
- WebView2 Runtime。大多数较新的 Windows 版本已经自带。

检查工具：

```powershell
node --version
pnpm --version
rustc --version
cargo --version
```

如果当前 PowerShell 找不到 Cargo，需要由开发者配置 Rust 的系统 PATH，并重新打开终端。项目不会自动修改 PATH。配置完成后再次执行：

```powershell
cargo --version
```

### macOS

需要安装 Node.js、pnpm、Xcode Command Line Tools 和 Rust。macOS 安装包必须在 macOS 主机或 macOS CI runner 上构建，不能在 Windows 主机直接生成可用的 DMG。

## 2. 安装依赖

在项目根目录执行：

```powershell
pnpm install --frozen-lockfile
```

如果没有 lockfile 或依赖版本确实需要更新，才使用 `pnpm install`，并检查生成的 `pnpm-lock.yaml` 是否应一并提交。

## 3. 启动开发调试

Windows PowerShell：

```powershell
cargo --version
pnpm tauri dev
```

Tauri 会启动 Vite 开发服务器并打开桌面窗口，默认开发地址是 `http://localhost:1420`。结束调试时先关闭窗口，再在终端按 `Ctrl+C`，避免 Cargo 或 Vite 进程继续占用端口和构建文件。

应用启动后，窗口顶部的网络状态会显示固定的 UDP 发现端口和实际 TCP 传输端口。UDP 发现端口为 `45731`；TCP 默认优先使用设置中的端口，如果被其他程序占用，会自动切换到空闲端口并继续广播实际传输端口。只有 UDP 和 TCP 都绑定成功时才会广播本机设备。

启动后建议先验证本机行为：

1. 未配对、没有在线同步目标时，复制和粘贴不应被应用读取、发送或回写。
2. 应用窗口关闭后应隐藏到系统托盘，而不是结束进程。
3. 在设置中切换简体中文和英文，重启后语言选择应保留。

### 常见启动问题

如果出现：

```text
failed to run 'cargo metadata' ... program not found
```

说明当前终端的 `PATH` 没有 Rust。请由开发者配置 Rust 的系统 PATH，重新打开终端后用 `cargo --version` 验证，不需要重新安装项目依赖。

如果提示 `1420` 端口被占用，先关闭另一个开发实例或旧的 Vite 进程，再重新启动：

```powershell
Get-NetTCPConnection -LocalPort 1420 -ErrorAction SilentlyContinue
```

如果出现 Cargo 文件锁，先退出仍在运行的 `lan-cross-sync.exe`、开发窗口和旧构建命令，再重试。

## 4. 自动化验证

Windows PowerShell 可以执行：

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --all -- --check
cargo check --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml
pnpm build
```

这些命令分别检查 Rust 格式、Rust 编译、Rust 单元测试和前端 TypeScript/Vite 构建。

## 5. 两台设备验收

两台设备连接同一个局域网，并确保 Windows 防火墙允许应用在专用网络通信。分别启动应用后按以下顺序验收：

1. 两台设备都出现在“发现的设备”列表。
2. 设备 A 生成配对码，设备 B 输入 6 位配对码并完成配对。
3. 两边的设备都进入已配对列表；重启其中一台后，连接状态可以恢复。
4. 复制文本和图片，确认另一台可以粘贴；关闭某个设备的接收剪贴板开关后，确认该设备不再被写入。
5. 传输单个文件、多个文件和嵌套目录，确认接收端的文件内容和目录结构正确。
6. 在传输过程中退出应用或断开网络，确认最终目录不会出现未完成的正式文件；重新启动后，索引中的临时目录会被清理。
7. 使用中英文界面分别走一遍配对、剪贴板和文件传输流程，确认错误提示可理解。

当前传输不支持断点续传。中断任务需要重新发起；新版本的临时文件不会作为最终文件暴露给用户。升级前遗留的旧版 `*.part` 文件不在新索引中，必要时可人工检查并清理。

## 6. Windows 打包

先确保 Cargo 已安装并加入系统 PATH，然后执行：

```powershell
cargo --version
pnpm tauri build --no-sign
```

生成的文件位于：

```text
src-tauri\target\release\lan-cross-sync.exe
src-tauri\target\release\bundle\nsis\lan-cross-sync_0.1.0_x64-setup.exe
src-tauri\target\release\bundle\msi\lan-cross-sync_0.1.0_x64_en-US.msi
```

文件用途：

- `target\release\lan-cross-sync.exe`：裸 Release 可执行文件，适合开发者快速验证，不是完整安装包。
- `*-setup.exe`：NSIS 安装包，适合普通用户下载安装，通常优先分发这个文件。
- `*.msi`：Windows Installer 包，适合企业部署、软件分发平台或需要 MSI 管理能力的场景。

也可以只生成一种格式：

```powershell
pnpm tauri build --no-sign --bundles nsis
pnpm tauri build --no-sign --bundles msi
```

`--no-sign` 生成未签名包。未签名程序在 Windows SmartScreen 或 macOS Gatekeeper 上可能出现警告；面向公众发布时需要配置代码签名证书。版本号来自 `src-tauri\tauri.conf.json`，发布新版本时同步更新 `package.json` 的版本号。

## 7. macOS 打包

macOS 上先安装所需 Rust 目标：

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm install --frozen-lockfile
```

生成 Universal 版本：

```bash
pnpm tauri build --no-sign --target universal-apple-darwin
```

如果不需要 Universal，也可以分别构建 Apple Silicon 和 Intel 版本：

```bash
pnpm tauri build --no-sign --target aarch64-apple-darwin
pnpm tauri build --no-sign --target x86_64-apple-darwin
```

DMG 和 `.app` 通常位于对应目标目录下的：

```text
src-tauri/target/<target>/release/bundle/dmg/
src-tauri/target/<target>/release/bundle/macos/
```

macOS 包应在目标系统上安装测试。要降低 Gatekeeper 拦截和首次运行提示，需要使用 Apple Developer ID 对应用签名并完成 notarization；当前仓库没有提交证书或 notarization 配置。

## 8. 发布前检查

- 在 Windows 和 macOS 分别生成对应平台安装包，不要把 Windows 的 `.exe` 或 `.msi` 当作 macOS 安装包。
- 在干净用户环境安装、启动、卸载一次，确认设置文件和托盘行为符合预期。
- 用两台真实设备完成发现、配对、重连、文本、图片、文件和中断清理验收。
- 发布未签名包时，在发布说明中明确说明系统安全提示；正式公开分发前完成 Windows 和 macOS 签名。
- 检查 `git status --short`，不要把 `target/`、`dist/` 或本地配置文件提交到仓库。
