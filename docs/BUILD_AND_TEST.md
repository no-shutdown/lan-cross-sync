# 构建、调试与打包

## 环境

- Windows 需要 Visual Studio Build Tools 的 `Desktop development with C++` 工作负载。
- 安装 Rust MSVC 工具链，并确认 `cargo --version` 可用。
- 安装 Node.js 和 pnpm。

首次安装依赖：

```powershell
pnpm install
```

## 本地验证

纯 Rust 测试：

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

前端类型检查和构建：

```powershell
pnpm build
```

启动桌面调试：

```powershell
pnpm tauri dev
```

启动后检查：

1. 两台同一局域网设备都运行应用，确认出现在“发现的设备”。
2. 设备 A 生成配对码，设备 B 输入 6 位配对码并点击配对。
3. 重启其中一台，确认已配对设备恢复在线状态。
4. 复制文本和图片，检查另一台设备的剪贴板；超大图片改用文件传输。
5. 选择在线目标设备，拖入文件或文件夹；接收端选择保存目录后检查目录结构和文件内容。
6. 关闭接收剪贴板开关，确认该设备不再写入远端剪贴板。

## 打包

生成当前平台发布包：

```powershell
pnpm tauri build
```

Windows 安装包输出在：

```text
src-tauri\target\release\bundle\nsis\
src-tauri\target\release\bundle\msi\
```

调试包不等于发布包。跨平台安装包需要在对应系统分别执行 `pnpm tauri build`；签名、 notarization 和 Windows 证书属于发布环境配置，不由仓库内默认配置代替。

## 诊断

如果出现 `cargo ... program not found`，关闭旧 PowerShell，重新打开终端后确认：

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cargo --version
rustc --version
```

如果出现 Cargo build directory 文件锁，先关闭仍在运行的 `lan-cross-sync` 调试实例，再重新执行构建。
