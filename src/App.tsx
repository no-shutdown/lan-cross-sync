import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { emitTo, listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow, WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { LogicalPosition } from '@tauri-apps/api/dpi'
import { cursorPosition } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import { isPermissionGranted, onAction, requestPermission, sendNotification } from '@tauri-apps/plugin-notification'
import './App.css'
import {
  acceptFileTransfer,
  cancelFileTransfer,
  cancelPairing,
  clearPairing,
  getAutostartEnabled,
  getDashboardState,
  requestPairing,
  setAutostartEnabled,
  setDefaultFileTarget,
  setDeviceName,
  setReceiveClipboard,
  setSendClipboard,
  setUiLocale,
  startFileTransfer,
  startPairing,
} from './lib/api'
import { normalizeLocale, t } from './lib/i18n'
import {
  DROP_HANDLE_LABEL,
  DROP_PANEL_LABEL,
  OVERLAY_EVENT_DRAG_ENTER,
  OVERLAY_EVENT_DRAG_STOP,
  OVERLAY_EVENT_DROP_COMPLETE,
  OVERLAY_EVENT_DROP_PATHS,
  OVERLAY_EVENT_PANEL_CLOSE,
  OVERLAY_EVENT_PANEL_ENTER,
  OVERLAY_EVENT_PANEL_LEAVE,
  OVERLAY_HANDLE_H,
  OVERLAY_HANDLE_W,
  OVERLAY_PANEL_H,
  OVERLAY_PANEL_W,
  handlePositionFromPanel,
  panelPositionFromHandle,
  type OverlayDropPayload,
} from './lib/overlay'
import type {
  DashboardState,
  DeviceId,
  DeviceInfo,
  Locale,
  PairedPeer,
  TransferEvent,
} from './lib/types'

function useTransparentOverlayDocument() {
  useEffect(() => {
    const html = document.documentElement
    const body = document.body
    const previous = {
      htmlBackground: html.style.background,
      htmlMinWidth: html.style.minWidth,
      htmlOverflow: html.style.overflow,
      bodyBackground: body.style.background,
      bodyMinWidth: body.style.minWidth,
      bodyOverflow: body.style.overflow,
    }

    html.style.background = 'transparent'
    html.style.minWidth = '0'
    html.style.overflow = 'hidden'
    body.style.background = 'transparent'
    body.style.minWidth = '0'
    body.style.overflow = 'hidden'

    return () => {
      html.style.background = previous.htmlBackground
      html.style.minWidth = previous.htmlMinWidth
      html.style.overflow = previous.htmlOverflow
      body.style.background = previous.bodyBackground
      body.style.minWidth = previous.bodyMinWidth
      body.style.overflow = previous.bodyOverflow
    }
  }, [])
}

type OverlayPosition = { x: number; y: number }

async function readLogicalWindowPosition(win: WebviewWindow): Promise<OverlayPosition> {
  const [position, scaleFactor] = await Promise.all([win.outerPosition(), win.scaleFactor()])
  const logicalPosition = position.toLogical(scaleFactor)
  return { x: logicalPosition.x, y: logicalPosition.y }
}

async function emitOverlayEvent<T>(target: string, event: string, payload?: T) {
  try {
    await emitTo(target, event, payload)
  } catch (err) {
    console.error(`failed to emit overlay event ${event}`, err)
  }
}

function initialHandlePosition(): OverlayPosition {
  const screen = window.screen as Screen & { availLeft?: number; availTop?: number }
  return {
    x: (screen.availLeft ?? 0) + screen.availWidth - OVERLAY_HANDLE_W,
    y: (screen.availTop ?? 0) + screen.availHeight - OVERLAY_HANDLE_H,
  }
}

export function DropHandle() {
  useTransparentOverlayDocument()

  const win = useMemo(() => getCurrentWebviewWindow(), [])
  const panelRef = useRef<WebviewWindow | null>(null)
  const panelLookupRef = useRef<Promise<WebviewWindow | null> | null>(null)
  const handlePositionRef = useRef<OverlayPosition | null>(null)
  const panelOpenRef = useRef(false)
  const pointerInsideHandleRef = useRef(false)
  const pointerInsidePanelRef = useRef(false)
  const draggingFileRef = useRef(false)
  const closeTimerRef = useRef<number | null>(null)
  const suppressHoverUntilRef = useRef(0)
  const windowOperationRef = useRef<Promise<void>>(Promise.resolve())
  const [draggingFile, setDraggingFile] = useState(false)

  const getPanel = useCallback(async () => {
    if (panelRef.current) return panelRef.current
    if (!panelLookupRef.current) {
      panelLookupRef.current = WebviewWindow.getByLabel(DROP_PANEL_LABEL)
    }
    const panel = await panelLookupRef.current
    if (panel) panelRef.current = panel
    return panel
  }, [])

  const enqueueWindowOperation = useCallback((operation: () => Promise<void>) => {
    const nextOperation = windowOperationRef.current.then(operation, operation)
    windowOperationRef.current = nextOperation.catch(() => {})
    return nextOperation
  }, [])

  const clearCloseTimer = useCallback(() => {
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current)
      closeTimerRef.current = null
    }
  }, [])

  const openPanel = useCallback((fromFileDrag = false) => {
    clearCloseTimer()
    if (panelOpenRef.current) return Promise.resolve()
    panelOpenRef.current = true

    return enqueueWindowOperation(async () => {
      try {
        const panel = await getPanel()
        const handlePosition = await readLogicalWindowPosition(win)
        if (!panel) throw new Error('drop panel window is unavailable')

        handlePositionRef.current = handlePosition
        const panelPosition = panelPositionFromHandle(handlePosition)
        await panel.setPosition(new LogicalPosition(panelPosition.x, panelPosition.y))
        await panel.show()
        // The panel occupies the handle area after it is shown, so the
        // native drop target is never resized or left in a half-hit-tested state.
        await win.hide()
        if (fromFileDrag) {
          await emitOverlayEvent(DROP_PANEL_LABEL, OVERLAY_EVENT_DRAG_ENTER)
        }
      } catch (err) {
        panelOpenRef.current = false
        console.error('failed to open drop panel window', err)
      }
    })
  }, [clearCloseTimer, enqueueWindowOperation, getPanel, win])

  const closePanel = useCallback(() => {
    clearCloseTimer()
    if (!panelOpenRef.current) return Promise.resolve()
    panelOpenRef.current = false

    return enqueueWindowOperation(async () => {
      if (panelOpenRef.current) return

      try {
        const panel = await getPanel()
        if (panel) {
          const panelPosition = await readLogicalWindowPosition(panel).catch(() => null)
          if (panelPosition) {
            handlePositionRef.current = handlePositionFromPanel(panelPosition)
          }
          await emitOverlayEvent(DROP_PANEL_LABEL, OVERLAY_EVENT_PANEL_CLOSE)
          await panel.hide()
        }

        const handlePosition = handlePositionRef.current ?? initialHandlePosition()
        await win.setPosition(new LogicalPosition(handlePosition.x, handlePosition.y))
        await win.show()
        suppressHoverUntilRef.current = Date.now() + 450
        setDraggingFile(false)
        draggingFileRef.current = false
      } catch (err) {
        console.error('failed to close drop panel window', err)
      }
    })
  }, [clearCloseTimer, enqueueWindowOperation, getPanel, win])

  const scheduleClose = useCallback((delay = 360) => {
    clearCloseTimer()
    if (!panelOpenRef.current) return

    closeTimerRef.current = window.setTimeout(() => {
      closeTimerRef.current = null
      if (!pointerInsideHandleRef.current && !pointerInsidePanelRef.current && !draggingFileRef.current) {
        void closePanel()
      }
    }, delay)
  }, [clearCloseTimer, closePanel])

  useEffect(() => {
    const position = initialHandlePosition()
    handlePositionRef.current = position
    void (async () => {
      try {
        await win.setPosition(new LogicalPosition(position.x, position.y))
        await win.show()
      } catch (err) {
        console.error('failed to initialize drop handle window', err)
      }
    })()

    return () => {
      clearCloseTimer()
    }
  }, [clearCloseTimer, win])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    void (async () => {
      try {
        const handler = await win.onDragDropEvent(async (event) => {
          if (event.payload.type === 'enter') {
            draggingFileRef.current = true
            setDraggingFile(true)
            clearCloseTimer()
            await openPanel(true)
          }

          if (event.payload.type === 'leave') {
            draggingFileRef.current = false
            setDraggingFile(false)
            await emitOverlayEvent(DROP_PANEL_LABEL, OVERLAY_EVENT_DRAG_STOP)
            if (!pointerInsideHandleRef.current && !pointerInsidePanelRef.current) scheduleClose(420)
          }

          if (event.payload.type === 'drop') {
            draggingFileRef.current = false
            setDraggingFile(false)
            await emitOverlayEvent<OverlayDropPayload>(DROP_PANEL_LABEL, OVERLAY_EVENT_DROP_PATHS, {
              paths: event.payload.paths,
            })
            await emitOverlayEvent(DROP_PANEL_LABEL, OVERLAY_EVENT_DRAG_STOP)
            if (!pointerInsideHandleRef.current && !pointerInsidePanelRef.current) scheduleClose(700)
          }
        })
        if (disposed) handler()
        else unlisten = handler
      } catch (err) {
        console.error('failed to register drop handle drag-and-drop listener', err)
      }
    })()

    return () => {
      disposed = true
      clearCloseTimer()
      unlisten?.()
    }
  }, [clearCloseTimer, openPanel, scheduleClose, win])

  useEffect(() => {
    let disposed = false
    const unlisteners: Array<() => void> = []

    void (async () => {
      try {
        const panelEnter = await listen<void>(OVERLAY_EVENT_PANEL_ENTER, () => {
          pointerInsidePanelRef.current = true
          clearCloseTimer()
        })
        const panelLeave = await listen<void>(OVERLAY_EVENT_PANEL_LEAVE, () => {
          pointerInsidePanelRef.current = false
          if (!pointerInsideHandleRef.current && !draggingFileRef.current) scheduleClose()
        })
        const dragEnter = await listen<void>(OVERLAY_EVENT_DRAG_ENTER, () => {
          draggingFileRef.current = true
          setDraggingFile(true)
          clearCloseTimer()
        })
        const dragStop = await listen<void>(OVERLAY_EVENT_DRAG_STOP, () => {
          draggingFileRef.current = false
          setDraggingFile(false)
          if (!pointerInsideHandleRef.current && !pointerInsidePanelRef.current) scheduleClose(500)
        })
        const dropComplete = await listen<void>(OVERLAY_EVENT_DROP_COMPLETE, () => {
          draggingFileRef.current = false
          setDraggingFile(false)
          if (!pointerInsideHandleRef.current && !pointerInsidePanelRef.current) scheduleClose(700)
        })
        unlisteners.push(panelEnter, panelLeave, dragEnter, dragStop, dropComplete)
        if (disposed) unlisteners.splice(0).forEach((unlisten) => unlisten())
      } catch (err) {
        console.error('failed to register drop panel coordination listener', err)
      }
    })()

    return () => {
      disposed = true
      unlisteners.forEach((unlisten) => unlisten())
    }
  }, [clearCloseTimer, scheduleClose])

  // Native cursor polling is the fallback for transparent windows where a
  // webview mouseleave event can be lost while the panel is being shown or
  // moved. It also makes the collapse rule independent of the underlying app.
  useEffect(() => {
    const timer = window.setInterval(() => {
      if (!panelOpenRef.current || draggingFileRef.current) return

      void (async () => {
        try {
          const panel = await getPanel()
          if (!panel || !panelOpenRef.current) return

          const [cursor, panelPosition, panelScale] = await Promise.all([
            cursorPosition(),
            panel.outerPosition(),
            panel.scaleFactor(),
          ])
          const panelWidth = OVERLAY_PANEL_W * panelScale
          const panelHeight = OVERLAY_PANEL_H * panelScale
          const insidePanel = cursor.x >= panelPosition.x
            && cursor.x <= panelPosition.x + panelWidth
            && cursor.y >= panelPosition.y
            && cursor.y <= panelPosition.y + panelHeight

          // The handle is hidden as soon as the panel is shown, so a missed
          // native mouseleave must not keep the old handle state alive.
          pointerInsideHandleRef.current = false
          pointerInsidePanelRef.current = insidePanel
          if (insidePanel) clearCloseTimer()
          else if (closeTimerRef.current === null) scheduleClose(360)
        } catch (err) {
          console.error('failed to track drop panel cursor position', err)
        }
      })()
    }, 120)

    return () => window.clearInterval(timer)
  }, [clearCloseTimer, getPanel, scheduleClose])

  const handlePointerEnter = useCallback(() => {
    pointerInsideHandleRef.current = true
    clearCloseTimer()
    if (Date.now() >= suppressHoverUntilRef.current) void openPanel()
  }, [clearCloseTimer, openPanel])

  const handlePointerLeave = useCallback(() => {
    pointerInsideHandleRef.current = false
    if (!draggingFileRef.current && !pointerInsidePanelRef.current) scheduleClose()
  }, [scheduleClose])

  const startWindowDrag = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    void win.startDragging().catch((err) => {
      console.error('failed to drag drop handle window', err)
    })
  }, [win])

  return (
    <div
      className={`overlay-handle-root ${draggingFile ? 'overlay-handle-dragging' : ''}`}
      onMouseEnter={handlePointerEnter}
      onMouseLeave={handlePointerLeave}
    >
      <button
        type="button"
        className="overlay-handle-button"
        aria-label={t('zh-CN', 'dropTitle')}
        title={t('zh-CN', 'dropTitle')}
        onPointerDown={startWindowDrag}
        onClick={() => void openPanel()}
      >
        <span className="overlay-handle-icon" aria-hidden="true">📂</span>
      </button>
    </div>
  )
}

export function DropPanel() {
  useTransparentOverlayDocument()

  const win = useMemo(() => getCurrentWebviewWindow(), [])
  const handleRef = useRef<WebviewWindow | null>(null)
  const handleLookupRef = useRef<Promise<WebviewWindow | null> | null>(null)
  const [dragOver, setDragOver] = useState(false)
  const [dashboard, setDashboard] = useState<DashboardState | null>(null)
  const [selectedTarget, setSelectedTarget] = useState<DeviceId | ''>('')
  const [error, setError] = useState<string | null>(null)
  const selectedTargetRef = useRef<DeviceId | ''>('')
  const localeRef = useRef<Locale>('zh-CN')

  const locale = normalizeLocale(dashboard?.settings.ui_locale)
  useEffect(() => { selectedTargetRef.current = selectedTarget }, [selectedTarget])
  useEffect(() => { localeRef.current = locale }, [locale])

  const getHandle = useCallback(async () => {
    if (handleRef.current) return handleRef.current
    if (!handleLookupRef.current) {
      handleLookupRef.current = WebviewWindow.getByLabel(DROP_HANDLE_LABEL)
    }
    const handle = await handleLookupRef.current
    if (handle) handleRef.current = handle
    return handle
  }, [])

  const emitHandleEvent = useCallback(async (event: string) => {
    await emitOverlayEvent(DROP_HANDLE_LABEL, event)
  }, [])

  const transferPaths = useCallback(async (paths: string[]) => {
    if (!paths.length) return
    if (!selectedTargetRef.current) {
      setError(t(localeRef.current, 'noTransferTarget'))
      return
    }

    try {
      await startFileTransfer(selectedTargetRef.current, paths)
    } catch (err) {
      setError(formatInvokeError(err, t(localeRef.current, 'errorTransfer')))
    }
  }, [])

  useEffect(() => {
    async function refresh() {
      try {
        setDashboard(await getDashboardState())
      } catch (err) {
        console.error('failed to refresh drop panel dashboard', err)
      }
    }
    void refresh()
    const timer = window.setInterval(refresh, 2000)
    return () => window.clearInterval(timer)
  }, [])

  useEffect(() => {
    if (!dashboard) return
    const onlinePeers = dashboard.paired_devices.filter((p) => p.state === 'connected')
    if (!onlinePeers.some((p) => p.device.id === selectedTargetRef.current)) {
      const defaultTarget = onlinePeers.find((p) => p.is_default_file_target)
      setSelectedTarget(defaultTarget?.device.id ?? onlinePeers[0]?.device.id ?? '')
    }
  }, [dashboard])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    void (async () => {
      try {
        const handler = await win.onMoved(async (event) => {
          try {
            const scaleFactor = await win.scaleFactor()
            const panelPosition = event.payload.toLogical(scaleFactor)
            const handle = await getHandle()
            if (handle) {
              const handlePosition = handlePositionFromPanel(panelPosition)
              await handle.setPosition(new LogicalPosition(handlePosition.x, handlePosition.y))
            }
          } catch (err) {
            console.error('failed to keep drop handle aligned with panel', err)
          }
        })
        if (disposed) handler()
        else unlisten = handler
      } catch (err) {
        console.error('failed to register drop panel move listener', err)
      }
    })()

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [getHandle, win])

  useEffect(() => {
    let disposed = false
    const unlisteners: Array<() => void> = []

    void (async () => {
      try {
        const panelClose = await listen<void>(OVERLAY_EVENT_PANEL_CLOSE, () => {
          setDragOver(false)
          setError(null)
        })
        const dragEnter = await listen<void>(OVERLAY_EVENT_DRAG_ENTER, () => {
          setDragOver(true)
        })
        const dragStop = await listen<void>(OVERLAY_EVENT_DRAG_STOP, () => {
          setDragOver(false)
        })
        const dropPaths = await listen<OverlayDropPayload>(OVERLAY_EVENT_DROP_PATHS, async (event) => {
          setDragOver(false)
          await transferPaths(event.payload.paths)
          await emitHandleEvent(OVERLAY_EVENT_DROP_COMPLETE)
        })
        unlisteners.push(panelClose, dragEnter, dragStop, dropPaths)
        if (disposed) unlisteners.splice(0).forEach((unlisten) => unlisten())
      } catch (err) {
        console.error('failed to register drop panel coordination listener', err)
      }
    })()

    return () => {
      disposed = true
      unlisteners.forEach((unlisten) => unlisten())
    }
  }, [emitHandleEvent, transferPaths])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    void (async () => {
      try {
        const handler = await win.onDragDropEvent(async (event) => {
          if (event.payload.type === 'enter') {
            setDragOver(true)
            await emitHandleEvent(OVERLAY_EVENT_DRAG_ENTER)
          }

          if (event.payload.type === 'leave') {
            setDragOver(false)
            await emitHandleEvent(OVERLAY_EVENT_DRAG_STOP)
          }

          if (event.payload.type === 'drop') {
            setDragOver(false)
            await transferPaths(event.payload.paths)
            await emitHandleEvent(OVERLAY_EVENT_DROP_COMPLETE)
          }
        })
        if (disposed) handler()
        else unlisten = handler
      } catch (err) {
        console.error('failed to register drop panel drag-and-drop listener', err)
      }
    })()

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [emitHandleEvent, transferPaths, win])

  const handlePointerEnter = useCallback(() => {
    void emitHandleEvent(OVERLAY_EVENT_PANEL_ENTER)
  }, [emitHandleEvent])

  const handlePointerLeave = useCallback(() => {
    void emitHandleEvent(OVERLAY_EVENT_PANEL_LEAVE)
  }, [emitHandleEvent])

  const startWindowDrag = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    void win.startDragging().catch((err) => {
      console.error('failed to drag drop panel window', err)
    })
  }, [win])

  const onlinePeers = dashboard?.paired_devices.filter((p) => p.state === 'connected') ?? []

  return (
    <div
      className={`overlay-panel-root ${dragOver ? 'overlay-panel-dragging' : ''}`}
      onMouseEnter={handlePointerEnter}
      onMouseLeave={handlePointerLeave}
    >
      <section className="overlay-card">
        <div
          className="overlay-header overlay-drag-handle"
          onPointerDown={startWindowDrag}
          title={t(locale, 'dropTitle')}
        >
          <span>{t(locale, 'targetDevice')}</span>
          <span className="overlay-drag-grip" aria-hidden="true">⠿</span>
        </div>
        <select
          className="overlay-select"
          value={selectedTarget}
          onChange={(event) => setSelectedTarget(event.target.value as DeviceId)}
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
      </section>
    </div>
  )
}

function formatInvokeError(err: unknown, fallback: string): string {
  if (err instanceof Error) return err.message
  if (typeof err === 'string') return err
  if (err && typeof err === 'object' && 'message' in err) {
    const message = (err as { message?: unknown }).message
    if (typeof message === 'string') return message
  }
  return fallback
}

function deviceStateLabel(locale: Locale, state: PairedPeer['state']): string {
  if (state === 'connected') return t(locale, 'online')
  if (state === 'discovered') return t(locale, 'discovered')
  if (state === 'pairing') return t(locale, 'pairingState')
  return t(locale, 'offline')
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  if (value < 1024 * 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(1)} MB`
  return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GB`
}

function notifyIncomingTransfer(locale: Locale, event: Extract<TransferEvent, { type: 'offer' }>) {
  try {
    sendNotification({
      title: t(locale, 'notificationIncomingTitle'),
      body: t(locale, 'notificationIncomingBody')
        .replace('{name}', event.peer.name)
        .replace('{count}', String(event.manifest.entries.length)),
      extra: { transferId: event.transfer_id },
    })
  } catch {
    // Notifications are unavailable in the browser preview or unsupported platforms.
  }
}

function backendError(locale: Locale, err: unknown, fallback: string): string {
  const raw = formatInvokeError(err, fallback)
  const known: Record<string, Parameters<typeof t>[1]> = {
    invalid_pairing_code: 'errorInvalidPairingCode',
    device_not_found: 'errorDeviceNotFound',
    device_endpoint_unavailable: 'errorDeviceEndpointUnavailable',
    no_active_pairing: 'errorNoActivePairing',
    invalid_code: 'errorInvalidCode',
    expired_code: 'errorExpiredCode',
    unpaired_peer: 'errorUnpairedPeer',
    invalid_device_name: 'errorInvalidDeviceName',
  }
  return known[raw] ? t(locale, known[raw]) : raw
}

function networkStatusText(locale: Locale, issueCode: string | null): string {
  const known: Record<string, Parameters<typeof t>[1]> = {
    network_not_ready: 'networkNotReady',
    network_discovery_unavailable: 'networkDiscoveryUnavailable',
    network_transport_unavailable: 'networkTransportUnavailable',
    network_services_unavailable: 'networkServicesUnavailable',
    transport_port_fallback: 'networkTransportFallback',
  }
  return t(locale, known[issueCode ?? ''] ?? 'networkReady')
}

function PeerCard({
  locale,
  peer,
  refresh,
  onError,
}: {
  locale: Locale
  peer: PairedPeer
  refresh: () => Promise<void>
  onError: (message: string) => void
}) {
  async function toggleReceiveClipboard() {
    try {
      await setReceiveClipboard(peer.device.id, !peer.receive_clipboard)
      await refresh()
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorTransfer')))
    }
  }

  async function toggleSendClipboard() {
    try {
      await setSendClipboard(peer.device.id, !peer.send_clipboard)
      await refresh()
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorTransfer')))
    }
  }

  async function makeDefaultTarget() {
    try {
      await setDefaultFileTarget(peer.device.id)
      await refresh()
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorTransfer')))
    }
  }

  async function removePairing() {
    try {
      await clearPairing(peer.device.id)
      await refresh()
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorTransfer')))
    }
  }

  return (
    <article className="peer-card">
      <div className="peer-details">
        <div className="peer-name-row">
          <h3>{peer.device.name}</h3>
          <span className={`status status-${peer.state}`}>{deviceStateLabel(locale, peer.state)}</span>
        </div>
        <code>{peer.device.id}</code>
      </div>
      <div className="peer-actions">
        <label className="check-row">
          <input type="checkbox" checked={peer.receive_clipboard} onChange={() => void toggleReceiveClipboard()} />
          {t(locale, 'receiveClipboard')}
        </label>
        <label className="check-row">
          <input type="checkbox" checked={peer.send_clipboard} onChange={() => void toggleSendClipboard()} />
          {t(locale, 'sendClipboard')}
        </label>
        <button onClick={() => void makeDefaultTarget()} disabled={peer.is_default_file_target}>
          {peer.is_default_file_target ? t(locale, 'defaultTarget') : t(locale, 'setFileTarget')}
        </button>
        <button className="danger" onClick={() => void removePairing()}>
          {t(locale, 'clearPairing')}
        </button>
      </div>
    </article>
  )
}

function DeviceNameEditor({
  locale,
  name,
  onSaved,
  onError,
}: {
  locale: Locale
  name: string
  onSaved: () => Promise<void>
  onError: (message: string) => void
}) {
  const [editing, setEditing] = useState(false)
  const [value, setValue] = useState(name)
  const [pending, setPending] = useState(false)

  useEffect(() => {
    if (!editing) setValue(name)
  }, [name, editing])

  async function save() {
    if (pending) return
    setPending(true)
    try {
      await setDeviceName(value)
      setEditing(false)
      await onSaved()
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorTransfer')))
    } finally {
      setPending(false)
    }
  }

  function cancel() {
    setValue(name)
    setEditing(false)
  }

  if (!editing) {
    return (
      <div className="device-name-row">
        <p className="device-caption">{name}</p>
        <button className="link-button" onClick={() => setEditing(true)}>
          {t(locale, 'renameDevice')}
        </button>
      </div>
    )
  }

  return (
    <div className="device-name-row">
      <input
        value={value}
        maxLength={40}
        placeholder={t(locale, 'renameDevicePlaceholder')}
        onChange={(event) => setValue(event.target.value)}
      />
      <button onClick={() => void save()} disabled={pending || value.trim().length === 0}>
        {t(locale, 'renameDeviceSave')}
      </button>
      <button onClick={cancel} disabled={pending}>
        {t(locale, 'renameDeviceCancel')}
      </button>
    </div>
  )
}

function DiscoveredDeviceCard({
  locale,
  device,
  onError,
  onPaired,
}: {
  locale: Locale
  device: DeviceInfo
  onError: (message: string) => void
  onPaired: () => Promise<void>
}) {
  const [code, setCode] = useState('')
  const [pending, setPending] = useState(false)

  async function pair() {
    if (pending) return
    setPending(true)
    try {
      await requestPairing(device.id, code)
      setCode('')
      await onPaired()
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorPairing')))
    } finally {
      setPending(false)
    }
  }

  return (
    <article className="peer-card discovered-card">
      <div className="peer-details">
        <div className="peer-name-row">
          <h3>{device.name}</h3>
          <span className="status status-discovered">{t(locale, 'availableToPair')}</span>
        </div>
        <code>{device.id}</code>
      </div>
      <div className="pair-input-row">
        <label>
          <span>{t(locale, 'pairingCode')}</span>
          <input
            value={code}
            inputMode="numeric"
            maxLength={6}
            pattern="[0-9]*"
            placeholder="000000"
            onChange={(event) => setCode(event.target.value.replace(/\D/g, '').slice(0, 6))}
          />
        </label>
        <button className="primary" onClick={() => void pair()} disabled={pending || code.length !== 6}>
          {pending ? t(locale, 'pairingPending') : t(locale, 'pair')}
        </button>
      </div>
    </article>
  )
}

function TransferPanel({
  locale,
  dashboard,
  events,
  onEvent,
  onError,
}: {
  locale: Locale
  dashboard: DashboardState
  events: TransferEvent[]
  onEvent: (event: TransferEvent) => void
  onError: (message: string) => void
}) {
  const onlinePeers = dashboard.paired_devices.filter((peer) => peer.state === 'connected')
  const defaultTarget = onlinePeers.find((peer) => peer.is_default_file_target)
  const [selectedTarget, setSelectedTarget] = useState<DeviceId | ''>(defaultTarget?.device.id ?? '')
  const [dropActive, setDropActive] = useState(false)
  const [incomingOffer, setIncomingOffer] = useState<Extract<TransferEvent, { type: 'offer' }> | null>(null)

  const targetId = selectedTarget || defaultTarget?.device.id || ''
  const target = onlinePeers.find((peer) => peer.device.id === targetId)
  const recentEvents = useMemo(() => events.slice(0, 5), [events])

  useEffect(() => {
    // A target that goes offline (or is unpaired) should fall back to the
    // placeholder instead of silently pointing at a device that can no
    // longer receive a transfer.
    if (selectedTarget && !onlinePeers.some((peer) => peer.device.id === selectedTarget)) {
      setSelectedTarget('')
    }
  }, [dashboard.paired_devices, selectedTarget])

  useEffect(() => {
    let disposed = false
    let unlistenTransfer: (() => void) | undefined
    let unlistenDrop: (() => void) | undefined

    void (async () => {
      try {
        unlistenTransfer = await listen<TransferEvent>('transfer-event', ({ payload }) => {
          onEvent(payload)
          if (payload.type === 'offer' && payload.direction === 'receiving') {
            setIncomingOffer(payload)
          }
        })
        unlistenDrop = await getCurrentWebviewWindow().onDragDropEvent((event) => {
          if (event.payload.type === 'enter') setDropActive(true)
          if (event.payload.type === 'leave' || event.payload.type === 'drop') setDropActive(false)
          if (event.payload.type === 'drop') void sendPaths(event.payload.paths)
        })
        if (disposed) {
          unlistenTransfer()
          unlistenDrop()
        }
      } catch {
        // The browser preview does not expose Tauri drag-and-drop events.
      }
    })()

    return () => {
      disposed = true
      unlistenTransfer?.()
      unlistenDrop?.()
    }
  }, [onEvent, targetId])

  async function sendPaths(paths: string[]) {
    if (!paths.length) return
    if (!targetId || target?.state !== 'connected') {
      onError(t(locale, 'noTransferTarget'))
      return
    }
    try {
      await startFileTransfer(targetId, paths)
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorTransfer')))
    }
  }

  async function chooseFiles() {
    try {
      const selected = await open({ multiple: true, directory: false })
      const paths = selected ? (Array.isArray(selected) ? selected : [selected]) : []
      await sendPaths(paths)
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorTransfer')))
    }
  }

  async function acceptIncoming() {
    if (!incomingOffer) return
    try {
      const destination = await open({ directory: true, multiple: false })
      if (typeof destination !== 'string') return
      await acceptFileTransfer(incomingOffer.transfer_id, destination)
      setIncomingOffer(null)
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorTransfer')))
    }
  }

  async function rejectIncoming() {
    if (!incomingOffer) return
    try {
      await cancelFileTransfer(incomingOffer.transfer_id)
      setIncomingOffer(null)
    } catch (err) {
      onError(backendError(locale, err, t(locale, 'errorTransfer')))
    }
  }

  return (
    <section className="panel transfer-panel">
      <div className="panel-title">
        <div>
          <h2>{t(locale, 'transfers')}</h2>
          <p>{target ? `${t(locale, 'targetDevice')}: ${target.device.name}` : t(locale, 'chooseTarget')}</p>
        </div>
        <label className="target-select">
          <span>{t(locale, 'targetDevice')}</span>
          <select value={targetId} onChange={(event) => setSelectedTarget(event.target.value)} required>
            {targetId === '' && (
              <option value="" disabled hidden>
                {t(locale, 'chooseTarget')}
              </option>
            )}
            {onlinePeers.map((peer) => (
              <option key={peer.device.id} value={peer.device.id}>
                {peer.device.name}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className={`drop-zone ${dropActive ? 'drop-zone-active' : ''}`}>
        <strong>{t(locale, 'dropTitle')}</strong>
        <span>{target?.state === 'connected' ? t(locale, 'selectFiles') : t(locale, 'dropHint')}</span>
        <button className="primary" onClick={() => void chooseFiles()} disabled={!targetId || target?.state !== 'connected'}>
          {t(locale, 'selectFiles')}
        </button>
      </div>

      {incomingOffer && (
        <div className="transfer-offer">
          <div>
            <strong>{t(locale, 'incomingOffer')}</strong>
            <span>
              {incomingOffer.peer.name} · {incomingOffer.manifest.entries.length} items ·{' '}
              {formatBytes(incomingOffer.manifest.total_bytes)}
            </span>
          </div>
          <div className="inline-actions">
            <button className="primary" onClick={() => void acceptIncoming()}>{t(locale, 'accept')}</button>
            <button className="danger" onClick={() => void rejectIncoming()}>
              {t(locale, 'reject')}
            </button>
          </div>
        </div>
      )}

      <div className="transfer-list">
        {recentEvents.length === 0 ? (
          <p>{t(locale, 'transferIdle')}</p>
        ) : (
          recentEvents.map((event, index) => (
            <div className="transfer-row" key={`${event.transfer_id}-${event.type}-${index}`}>
              <span className="transfer-name">{event.transfer_id.slice(0, 8)}</span>
              {event.type === 'progress' ? (
                <progress value={event.transferred_bytes} max={Math.max(event.total_bytes, 1)} />
              ) : (
                <span className={`transfer-state transfer-${event.type}`}>
                  {event.type === 'offer'
                    ? event.direction === 'sending'
                      ? t(locale, 'sending')
                      : t(locale, 'incomingOffer')
                    : t(locale, event.type)}
                </span>
              )}
              {event.type === 'progress' && (
                <span>{formatBytes(event.transferred_bytes)}</span>
              )}
            </div>
          ))
        )}
      </div>
    </section>
  )
}

export default function App() {
  const [dashboard, setDashboard] = useState<DashboardState | null>(null)
  const [autostart, setAutostart] = useState<boolean | null>(null)
  const [autostartPending, setAutostartPending] = useState(false)
  const [events, setEvents] = useState<TransferEvent[]>([])
  const [error, setError] = useState<string | null>(null)
  const locale = normalizeLocale(dashboard?.settings.ui_locale)

  async function refresh() {
    let nextError: string | null = null
    try {
      setDashboard(await getDashboardState())
    } catch (err) {
      nextError = backendError(locale, err, t(locale, 'errorLoadDashboard'))
    }
    try {
      setAutostart(await getAutostartEnabled())
    } catch (err) {
      nextError ??= backendError(locale, err, t(locale, 'errorTransfer'))
    }
    setError(nextError)
  }

  async function toggleAutostart() {
    if (autostart === null || autostartPending) return
    setAutostartPending(true)
    try {
      setAutostart(await setAutostartEnabled(!autostart))
      setError(null)
    } catch (err) {
      setError(backendError(locale, err, t(locale, 'errorTransfer')))
    } finally {
      setAutostartPending(false)
    }
  }

  async function updateLocale(nextLocale: Locale) {
    try {
      await setUiLocale(nextLocale)
      await refresh()
    } catch (err) {
      setError(formatInvokeError(err, t(locale, 'errorTransfer')))
    }
  }

  async function beginPairing() {
    try {
      await startPairing()
      await refresh()
    } catch (err) {
      setError(backendError(locale, err, t(locale, 'errorPairing')))
    }
  }

  async function stopPairing() {
    try {
      await cancelPairing()
      await refresh()
    } catch (err) {
      setError(backendError(locale, err, t(locale, 'errorPairing')))
    }
  }

  function onTransferEvent(event: TransferEvent) {
    const terminal = event.type === 'completed' || event.type === 'failed' || event.type === 'cancelled'
    setEvents((current) => [event, ...current.filter((item) => item.transfer_id !== event.transfer_id || (!terminal && item.type !== event.type))].slice(0, 12))
    if (event.type === 'offer' && event.direction === 'receiving') {
      notifyIncomingTransfer(locale, event)
    }
  }

  useEffect(() => {
    void refresh()
    const timer = window.setInterval(() => void refresh(), 2_000)
    return () => window.clearInterval(timer)
  }, [])

  useEffect(() => {
    let disposed = false
    let unlistenAction: (() => void) | undefined

    void (async () => {
      try {
        let granted = await isPermissionGranted()
        if (!granted) {
          granted = (await requestPermission()) === 'granted'
        }
      } catch {
        // Notifications are unavailable in the browser preview or unsupported platforms.
      }
      try {
        const listener = await onAction(() => {
          const appWindow = getCurrentWebviewWindow()
          void appWindow.show()
          void appWindow.setFocus()
        })
        if (disposed) {
          listener.unregister()
        } else {
          unlistenAction = () => listener.unregister()
        }
      } catch {
        // ignore
      }
    })()

    return () => {
      disposed = true
      unlistenAction?.()
    }
  }, [])

  if (!dashboard) {
    return (
      <main className="shell loading-state">
        <div className="spinner" />
        <p>{t(locale, 'appName')}</p>
      </main>
    )
  }

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">{t(locale, 'localDevice')}</p>
          <h1>{t(locale, 'appName')}</h1>
          <DeviceNameEditor
            locale={locale}
            name={dashboard.settings.local_device.name}
            onSaved={refresh}
            onError={setError}
          />
        </div>
        <div className="topbar-actions">
          <label className="language-select">
            <span>{t(locale, 'language')}</span>
            <select value={locale} onChange={(event) => void updateLocale(event.target.value as Locale)}>
              <option value="zh-CN">简体中文</option>
              <option value="en-US">English</option>
            </select>
          </label>
          <button onClick={() => void refresh()}>{t(locale, 'refresh')}</button>
        </div>
      </header>

      {error && (
        <section className="error">
          <span>{error}</span>
          <button className="error-dismiss" onClick={() => setError(null)} aria-label={t(locale, 'cancel')}>
            ×
          </button>
        </section>
      )}
      {dashboard.pairing_error_code && (
        <section className="error">{backendError(locale, dashboard.pairing_error_code, t(locale, 'errorPairing'))}</section>
      )}

      <section
        className={`network-status ${
          dashboard.network_status.advertising
            ? dashboard.network_status.issue_code
              ? 'network-status-warning'
              : 'network-status-ready'
            : 'network-status-error'
        }`}
        aria-live="polite"
      >
        <div className="network-status-copy">
          <div className="network-status-title">
            <h2>{t(locale, 'networkStatus')}</h2>
            <span className="status">
              {dashboard.network_status.advertising ? t(locale, 'advertising') : t(locale, 'notAdvertising')}
            </span>
          </div>
          <p>{networkStatusText(locale, dashboard.network_status.issue_code)}</p>
        </div>
        <div className="network-endpoints">
          <span>
            <strong>{t(locale, 'udpPort')}</strong> {dashboard.network_status.discovery_port}
          </span>
          <span>
            <strong>{t(locale, 'tcpPort')}</strong> {dashboard.network_status.transport_port ?? '-'}
          </span>
        </div>
      </section>

      <section className="panel">
        <div className="panel-title">
          <div>
            <h2>{t(locale, 'pairing')}</h2>
            <p>{dashboard.active_pairing_code ? t(locale, 'pairingCodeHint') : t(locale, 'noActivePairing')}</p>
          </div>
          {dashboard.active_pairing_code ? (
            <button onClick={() => void stopPairing()}>{t(locale, 'cancel')}</button>
          ) : (
            <button className="primary" onClick={() => void beginPairing()}>{t(locale, 'startPairing')}</button>
          )}
        </div>
        {dashboard.active_pairing_code && <div className="pairing-code">{dashboard.active_pairing_code}</div>}
      </section>

      <TransferPanel
        locale={locale}
        dashboard={dashboard}
        events={events}
        onEvent={onTransferEvent}
        onError={setError}
      />

      <section className="panel">
        <div className="panel-title">
          <div>
            <h2>{t(locale, 'pairedDevices')}</h2>
            <p>{dashboard.paired_devices.length}</p>
          </div>
        </div>
        {dashboard.paired_devices.length === 0 ? (
          <p>{t(locale, 'noPairedDevices')}</p>
        ) : (
          <div className="peer-list">
            {dashboard.paired_devices.map((peer) => (
              <PeerCard key={peer.device.id} locale={locale} peer={peer} refresh={refresh} onError={setError} />
            ))}
          </div>
        )}
      </section>

      <section className="panel">
        <div className="panel-title">
          <div>
            <h2>{t(locale, 'discoveredDevices')}</h2>
            <p>{dashboard.discovered_devices.length}</p>
          </div>
        </div>
        {dashboard.discovered_devices.length === 0 ? (
          <p>{t(locale, 'noDiscoveredDevices')}</p>
        ) : (
          <div className="peer-list">
            {dashboard.discovered_devices.map((device) => (
              <DiscoveredDeviceCard
                key={device.id}
                locale={locale}
                device={device}
                onError={setError}
                onPaired={refresh}
              />
            ))}
          </div>
        )}
      </section>

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
