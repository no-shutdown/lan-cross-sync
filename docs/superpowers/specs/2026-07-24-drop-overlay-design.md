# 悬浮投送区 设计文档

**日期**: 2026-07-24  
**状态**: 已批准

---

## 背景

当前的文件传输流程要求用户打开主窗口，将文件拖到窗口内的 drop-zone 区域。用户希望无需打开主窗口，只要把文件拖向屏幕右下角，悬浮投送区自动弹出，松手完成发送。

---

## 目标

- 拖动文件时，屏幕右下角自动出现悬浮投送窗口
- 未拖动时完全不可见，不干扰正常桌面操作
- 悬浮窗内可选择目标设备，投送完成后自动收起

---

## 设计方案：边缘触发 + 第二窗口

### 两个运行时窗口

| 窗口 | label | 用途 |
|------|-------|------|
| 主窗口 | `main` | 现有 UI，不变 |
| 投送叠层 | `drop-overlay` | 悬浮投送区，新增 |

### drop-overlay 窗口属性

在 `tauri.conf.json` 中定义静态属性（位置不能写表达式，在前端初始化时修正）：

```json
{
  "label": "drop-overlay",
  "title": "",
  "width": 80,
  "height": 80,
  "decorations": false,
  "transparent": true,
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "visible": true,
  "resizable": false
}
```

**初始定位**：`DropOverlay` 组件 mount 时，通过 `window.screen.availWidth` / `availHeight`（浏览器 API，已排除任务栏）计算右下角坐标，调用 `webviewWindow.setPosition()` 定位。

### 两个状态

**状态一：待机（idle）**

- 窗口尺寸：80 × 80 px，定位在右下角
- 内容：完全透明，无任何可见元素
- `setIgnoreCursorEvents(true)`：鼠标点击直接穿透，不拦截其他应用
- Tauri `onDragDropEvent` 仍然有效（OS 拖放协议独立于鼠标点击事件）

**状态二：激活（active）**

触发条件：`onDragDropEvent` 收到 `enter` 事件

1. 取消任何待执行的收起定时器
2. `setIgnoreCursorEvents(false)`
3. 窗口扩展至 220 × 180 px，同步将位置向左上偏移（Δx = -140, Δy = -100），保持右下角坐标不变
4. 渲染完整 UI：设备选择器 + 拖放区 + 高亮动画（slide-up 入场）

收起条件：`leave` 或 `drop` 事件触发后，启动 300ms 定时器；若定时器期间再次收到 `enter`，则取消定时器（防止快速移入移出导致闪烁）；定时器到期后执行 fade 出场动画，随后恢复窗口尺寸与位置至待机值，并重新 `setIgnoreCursorEvents(true)`。

---

## 数据流

```
drop-overlay 窗口
  │
  ├── 启动时：调用 getDashboardState() 取在线设备列表
  ├── 每 2s：轮询刷新设备列表（与主窗口相同机制）
  ├── 用户拖入文件 → onDragDropEvent(drop)
  │     └── 调用 startFileTransfer(targetId, paths)
  └── 显示成功/失败 toast，300ms 后收起
```

选中的目标设备保存在 overlay 自己的 React state 中，默认值为 `is_default_file_target` 的设备。

---

## 前端结构变更

### App.tsx 分发逻辑

```tsx
// 顶部加入
const windowLabel = getCurrentWebviewWindow().label

if (windowLabel === 'drop-overlay') {
  return <DropOverlay />
}
// 以下是现有的主窗口 UI
```

### DropOverlay 组件职责

- 调用 `getDashboardState()` 轮询在线设备
- 监听 `onDragDropEvent`，驱动 idle ↔ active 状态切换
- 调用 `webviewWindow.setSize()` / `setPosition()` 实现窗口缩放
- 调用 `webviewWindow.setIgnoreCursorEvents()` 切换穿透模式
- 调用 `startFileTransfer()` 执行发送

### 新增 CSS

overlay 样式独立于主窗口样式，包含：
- `.overlay-root` — 透明背景容器
- `.overlay-card` — 半透明深色卡片（激活态）
- `.overlay-dropzone` — 拖放区，拖入时高亮
- 进出场动画（slide-up / fade）

---

## 文件变更清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `src-tauri/tauri.conf.json` | 修改 | 新增 drop-overlay 窗口定义 |
| `src-tauri/capabilities/desktop.json` | 修改 | 将 drop-overlay 加入 windows 列表 |
| `src/App.tsx` | 修改 | 顶部加 window label 判断，末尾加 DropOverlay 组件 |
| `src/App.css` | 修改 | 新增 overlay 相关样式 |

不需要新增 Vite 入口、不需要修改 Rust 后端命令，复用现有所有 API。

---

## 边界情况

| 场景 | 处理方式 |
|------|---------|
| 无在线配对设备 | 设备选择器显示"无可用设备"，拖放区禁用，投送后报错 |
| 投送中再次拖入 | 忽略，或追加到队列（与主窗口现有行为一致）|
| 主窗口关闭 | overlay 窗口继续独立运行 |
| 多显示器 | 使用主显示器（primary monitor）的工作区计算位置 |
| 拖动文件夹 | 与主窗口一致，startFileTransfer 已支持 |

---

## 不在此次范围内

- overlay 窗口位置由用户自由拖拽调整（可后续迭代）
- 从系统托盘触发 overlay
- 多目标同时投送
