# 悬浮投送区 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在屏幕右下角添加一个透明感应角，拖动文件到该区域时弹出带设备选择器的悬浮投送窗口，松手完成发送后自动收起。

**Architecture:** 新增第二个 Tauri 窗口 `drop-overlay`（always-on-top、无边框、透明），与主窗口共享同一 React bundle，通过 `main.tsx` 按 window label 路由到 `DropOverlay` 组件。待机时窗口为 80×80px 透明角块、鼠标穿透；拖入时扩展为 220×180px 并显示完整 UI。

**Tech Stack:** Tauri v2, React 18, TypeScript, `@tauri-apps/api/dpi` (LogicalPosition / LogicalSize)

---

## 文件变更

| 文件 | 操作 |
|------|------|
| `src-tauri/tauri.conf.json` | 修改：新增 drop-overlay 窗口 |
| `src-tauri/capabilities/desktop.json` | 修改：drop-overlay 加入 windows 列表 |
| `src/App.css` | 修改：新增 overlay 相关样式 |
| `src/App.tsx` | 修改：新增 DropOverlay 组件（具名导出）+ 新增 import |
| `src/main.tsx` | 修改：按 window label 路由到 DropOverlay 或 App |

---

## Task 1: Tauri 窗口配置

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/desktop.json`

- [ ] **Step 1: 在 tauri.conf.json 的 windows 数组中追加 drop-overlay 窗口**

将 `src-tauri/tauri.conf.json` 中的 `app.windows` 数组从：

```json
"windows": [
  {
    "title": "lan-cross-sync",
    "width": 800,
    "height": 600,
    "minWidth": 640,
    "minHeight": 520
  }
]
```

改为：

```json
"windows": [
  {
    "title": "lan-cross-sync",
    "width": 800,
    "height": 600,
    "minWidth": 640,
    "minHeight": 520
  },
  {
    "label": "drop-overlay",
    "title": "",
    "width": 80,
    "height": 80,
    "decorations": false,
    "transparent": true,
    "alwaysOnTop": true,
    "skipTaskbar": true,
    "visible": false,
    "focus": false,
    "resizable": false
  }
]
```

- [ ] **Step 2: 将 drop-overlay 加入 capabilities**

将 `src-tauri/capabilities/desktop.json` 中的 `windows` 数组从：

```json
"windows": ["main"]
```

改为：

```json
"windows": ["main", "drop-overlay"]
```

- [ ] **Step 3: 验证 JSON 语法**

```bash
node -e "JSON.parse(require('fs').readFileSync('src-tauri/tauri.conf.json','utf8')); console.log('tauri.conf.json OK')"
node -e "JSON.parse(require('fs').readFileSync('src-tauri/capabilities/desktop.json','utf8')); console.log('desktop.json OK')"
```

预期：两行均输出 `OK`。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/capabilities/desktop.json
git commit -m "feat: add drop-overlay window config"
```

---

## Task 2: Overlay CSS

**Files:**
- Modify: `src/App.css`

- [ ] **Step 1: 在 App.css 末尾追加 overlay 样式**

```css
/* ── Drop Overlay Window ── */

.overlay-root {
  width: 100vw;
  height: 100vh;
  background: transparent;
  display: flex;
  align-items: flex-end;
  justify-content: flex-end;
  pointer-events: none;
}

.overlay-root.overlay-active {
  pointer-events: auto;
}

.overlay-card {
  width: 196px;
  background: rgba(15, 23, 42, 0.92);
  border: 1px solid rgba(99, 102, 241, 0.45);
  border-radius: 12px;
  padding: 12px;
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.45);
  animation: overlay-slide-up 0.18s ease-out;
}

.overlay-card.overlay-card-dragover {
  border-color: rgba(99, 102, 241, 0.9);
  box-shadow: 0 0 0 2px rgba(99, 102, 241, 0.25), 0 8px 32px rgba(0, 0, 0, 0.45);
}

.overlay-header {
  font-size: 10px;
  color: #64748b;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  margin-bottom: 6px;
}

.overlay-select {
  width: 100%;
  background: #1e293b;
  border: 1px solid #334155;
  color: #e2e8f0;
  border-radius: 6px;
  padding: 5px 8px;
  font-size: 12px;
  margin-bottom: 8px;
  cursor: pointer;
  outline: none;
}

.overlay-select:focus {
  border-color: #6366f1;
}

.overlay-dropzone {
  border: 1.5px dashed #475569;
  border-radius: 8px;
  padding: 12px 8px;
  text-align: center;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  transition: border-color 0.12s, background 0.12s;
}

.overlay-dropzone.overlay-dropzone-active {
  border-color: #6366f1;
  background: rgba(99, 102, 241, 0.1);
}

.overlay-drop-icon {
  font-size: 22px;
  line-height: 1;
}

.overlay-drop-label {
  font-size: 11px;
  color: #94a3b8;
  line-height: 1.3;
}

.overlay-error {
  font-size: 10px;
  color: #f87171;
  margin-top: 6px;
  text-align: center;
  word-break: break-all;
}

@keyframes overlay-slide-up {
  from { opacity: 0; transform: translateY(10px); }
  to   { opacity: 1; transform: translateY(0); }
}
```

- [ ] **Step 2: TypeScript 构建验证**

```bash
pnpm build
```

预期：构建成功，无报错。

- [ ] **Step 3: Commit**

```bash
git add src/App.css
git commit -m "feat: add drop-overlay CSS styles"
```

---

## Task 3: DropOverlay 组件

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: 更新 App.tsx 第 1 行的 react import，加入 useCallback 和 useRef**

将：

```tsx
import { useEffect, useMemo, useState } from 'react'
```

改为：

```tsx
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
```

- [ ] **Step 2: 在 `import { getCurrentWebviewWindow }` 那行之后新增一行 dpi import**

当前第 3 行是：

```tsx
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
```

在其**之后**插入：

```tsx
import { LogicalPosition, LogicalSize } from '@tauri-apps/api/dpi'
```

- [ ] **Step 3: 在 `formatInvokeError` 函数定义之前插入 DropOverlay 组件，并在函数末尾加具名导出**

在 `function formatInvokeError` 的上方（当前约第 34 行）插入以下完整函数。注意：`formatInvokeError` 是函数声明，JS 会提升，DropOverlay 内部可安全调用它。

```tsx
export function DropOverlay() {
  const IDLE_W = 80
  const IDLE_H = 80
  const ACTIVE_W = 220
  const ACTIVE_H = 180

  const [active, setActive] = useState(false)
  const [dragOver, setDragOver] = useState(false)
  const [dashboard, setDashboard] = useState<DashboardState | null>(null)
  const [selectedTarget, setSelectedTarget] = useState<DeviceId | ''>('')
  const [error, setError] = useState<string | null>(null)

  const collapseTimerRef = useRef<number | null>(null)
  const selectedTargetRef = useRef<DeviceId | ''>('')
  useEffect(() => { selectedTargetRef.current = selectedTarget }, [selectedTarget])

  const win = useMemo(() => getCurrentWebviewWindow(), [])

  // 让 html/body 透明，使 Tauri transparent 窗口背景露出
  useEffect(() => {
    document.documentElement.style.background = 'transparent'
    document.body.style.background = 'transparent'
  }, [])

  // 定位到右下角，设置鼠标穿透，然后显示窗口
  useEffect(() => {
    const x = window.screen.availWidth - IDLE_W
    const y = window.screen.availHeight - IDLE_H
    async function init() {
      await win.setPosition(new LogicalPosition(x, y))
      await win.setIgnoreCursorEvents(true)
      await win.show()
    }
    void init()
  }, [win])

  // 每 2s 轮询设备列表
  useEffect(() => {
    async function refresh() {
      try {
        setDashboard(await getDashboardState())
      } catch {}
    }
    void refresh()
    const timer = window.setInterval(refresh, 2000)
    return () => window.clearInterval(timer)
  }, [])

  // 保持 selectedTarget 指向有效的在线设备
  useEffect(() => {
    if (!dashboard) return
    const onlinePeers = dashboard.paired_devices.filter((p) => p.state === 'connected')
    if (!onlinePeers.some((p) => p.device.id === selectedTargetRef.current)) {
      const defaultTarget = onlinePeers.find((p) => p.is_default_file_target)
      setSelectedTarget(defaultTarget?.device.id ?? onlinePeers[0]?.device.id ?? '')
    }
  }, [dashboard])

  const expand = useCallback(async () => {
    const idleX = window.screen.availWidth - IDLE_W
    const idleY = window.screen.availHeight - IDLE_H
    await win.setIgnoreCursorEvents(false)
    await win.setSize(new LogicalSize(ACTIVE_W, ACTIVE_H))
    await win.setPosition(new LogicalPosition(idleX - (ACTIVE_W - IDLE_W), idleY - (ACTIVE_H - IDLE_H)))
    setActive(true)
  }, [win])

  const collapse = useCallback(async () => {
    setActive(false)
    setDragOver(false)
    setError(null)
    const idleX = window.screen.availWidth - IDLE_W
    const idleY = window.screen.availHeight - IDLE_H
    await win.setSize(new LogicalSize(IDLE_W, IDLE_H))
    await win.setPosition(new LogicalPosition(idleX, idleY))
    await win.setIgnoreCursorEvents(true)
  }, [win])

  // 注册拖放事件监听，只注册一次
  useEffect(() => {
    let isExpanded = false
    let unlisten: (() => void) | undefined

    void (async () => {
      try {
        unlisten = await win.onDragDropEvent(async (event) => {
          if (event.payload.type === 'enter') {
            if (collapseTimerRef.current !== null) {
              window.clearTimeout(collapseTimerRef.current)
              collapseTimerRef.current = null
            }
            if (!isExpanded) {
              isExpanded = true
              await expand()
            }
            setDragOver(true)
          }

          if (event.payload.type === 'leave') {
            setDragOver(false)
            collapseTimerRef.current = window.setTimeout(async () => {
              collapseTimerRef.current = null
              isExpanded = false
              await collapse()
            }, 300)
          }

          if (event.payload.type === 'drop') {
            setDragOver(false)
            const { paths } = event.payload
            if (paths.length > 0 && selectedTargetRef.current) {
              try {
                await startFileTransfer(selectedTargetRef.current, paths)
              } catch (err) {
                setError(formatInvokeError(err, 'Transfer failed'))
              }
            }
            collapseTimerRef.current = window.setTimeout(async () => {
              collapseTimerRef.current = null
              isExpanded = false
              await collapse()
            }, 500)
          }
        })
      } catch {}
    })()

    return () => {
      if (collapseTimerRef.current !== null) window.clearTimeout(collapseTimerRef.current)
      unlisten?.()
    }
  }, [win, expand, collapse])

  const onlinePeers = dashboard?.paired_devices.filter((p) => p.state === 'connected') ?? []
  const locale = normalizeLocale(dashboard?.settings.ui_locale)

  return (
    <div className={`overlay-root ${active ? 'overlay-active' : ''}`}>
      {active && (
        <div className={`overlay-card ${dragOver ? 'overlay-card-dragover' : ''}`}>
          <div className="overlay-header">{t(locale, 'targetDevice')}</div>
          <select
            className="overlay-select"
            value={selectedTarget}
            onChange={(e) => setSelectedTarget(e.target.value as DeviceId)}
          >
            {onlinePeers.length === 0 && (
              <option value="" disabled>{t(locale, 'noTransferTarget')}</option>
            )}
            {onlinePeers.map((peer) => (
              <option key={peer.device.id} value={peer.device.id}>
                {peer.device.name}
              </option>
            ))}
          </select>
          <div className={`overlay-dropzone ${dragOver ? 'overlay-dropzone-active' : ''}`}>
            <span className="overlay-drop-icon">📂</span>
            <span className="overlay-drop-label">{t(locale, 'dropTitle')}</span>
          </div>
          {error && <div className="overlay-error">{error}</div>}
        </div>
      )}
    </div>
  )
}
```

- [ ] **Step 4: TypeScript 构建验证**

```bash
pnpm build
```

预期：构建成功，无报错。若有类型错误：
- `Cannot find name 'LogicalPosition'` → 确认 Step 2 的 import 已添加
- `Cannot find name 'useCallback'` → 确认 Step 1 的 react import 已更新

- [ ] **Step 5: Commit**

```bash
git add src/App.tsx
git commit -m "feat: add DropOverlay component"
```

---

## Task 4: 路由 — main.tsx

**Files:**
- Modify: `src/main.tsx`

- [ ] **Step 1: 替换 main.tsx 全部内容**

将 `src/main.tsx` 改为：

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { DropOverlay } from "./App";

const _windowLabel = getCurrentWebviewWindow().label;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {_windowLabel === "drop-overlay" ? <DropOverlay /> : <App />}
  </React.StrictMode>,
);
```

- [ ] **Step 2: TypeScript 构建验证**

```bash
pnpm build
```

预期：构建成功，无报错。

- [ ] **Step 3: Commit**

```bash
git add src/main.tsx
git commit -m "feat: route drop-overlay window to DropOverlay component"
```

---

## Task 5: 手动冒烟测试

- [ ] **Step 1: 启动开发环境**

```bash
pnpm tauri dev
```

预期：主窗口正常打开；任务栏不出现第二个图标；屏幕右下角无可见元素。

- [ ] **Step 2: 验证鼠标穿透（待机状态）**

在屏幕最右下角 80×80px 区域单击，点击应穿透到桌面或后方窗口，不触发任何 UI。

- [ ] **Step 3: 验证拖入触发**

打开资源管理器，选取任意文件并开始拖动，将鼠标移向屏幕右下角。

预期：
- 拖入触发区后，悬浮卡片从右下角向上弹出（slide-up 动画）
- 卡片含"目标设备"标签、设备下拉选择器、拖放区（📂 图标 + 提示文字）

- [ ] **Step 4: 验证拖出收起**

将文件从右下角拖走（不释放）。

预期：约 300ms 后悬浮卡片消失，恢复透明待机状态。再次拖入应能再次弹出。

- [ ] **Step 5: 验证文件投送**

确保对端设备在线并已配对，将文件拖到弹出的卡片拖放区后释放。

预期：文件开始传输；悬浮卡片约 500ms 后自动收起；主窗口 Transfer 面板可见进度。

- [ ] **Step 6: 验证无在线设备**

断开所有配对设备后，再次拖文件到右下角。

预期：卡片弹出，下拉选择器显示"请先选择在线的已配对设备"（禁用状态）；松开文件后卡片显示错误信息后收起。

- [ ] **Step 7: 修复问题（若有）并 commit**

```bash
git add src/App.tsx src/App.css src/main.tsx
git commit -m "fix: <描述问题>"
```

---

## 完成标准

- [ ] `pnpm build` 零报错
- [ ] 待机：右下角完全透明，鼠标点击穿透
- [ ] 激活：悬浮卡片从右下角弹出，含设备选择器和拖放区
- [ ] 投送：文件传输启动，500ms 后自动收起
- [ ] 拖出：300ms 后自动收起，重新拖入可再次弹出
- [ ] 无设备：显示占位提示，不崩溃
