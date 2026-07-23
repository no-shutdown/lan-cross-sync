import { useEffect, useMemo, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { open } from '@tauri-apps/plugin-dialog'
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
  setReceiveClipboard,
  setUiLocale,
  startFileTransfer,
  startPairing,
} from './lib/api'
import { normalizeLocale, t } from './lib/i18n'
import type {
  DashboardState,
  DeviceId,
  DeviceInfo,
  Locale,
  PairedPeer,
  TransferEvent,
} from './lib/types'

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
  async function toggleClipboard() {
    try {
      await setReceiveClipboard(peer.device.id, !peer.receive_clipboard)
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
          <input type="checkbox" checked={peer.receive_clipboard} onChange={() => void toggleClipboard()} />
          {t(locale, 'receiveClipboard')}
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
        <button onClick={() => void pair()} disabled={pending || code.length !== 6}>
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
  const defaultTarget = dashboard.paired_devices.find((peer) => peer.is_default_file_target)
  const [selectedTarget, setSelectedTarget] = useState<DeviceId | ''>(defaultTarget?.device.id ?? '')
  const [dropActive, setDropActive] = useState(false)
  const [incomingOffer, setIncomingOffer] = useState<Extract<TransferEvent, { type: 'offer' }> | null>(null)

  const targetId = selectedTarget || defaultTarget?.device.id || ''
  const target = dashboard.paired_devices.find((peer) => peer.device.id === targetId)
  const recentEvents = useMemo(() => events.slice(0, 5), [events])

  useEffect(() => {
    if (selectedTarget && !dashboard.paired_devices.some((peer) => peer.device.id === selectedTarget)) {
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
          <select value={targetId} onChange={(event) => setSelectedTarget(event.target.value)}>
            <option value="">{t(locale, 'chooseTarget')}</option>
            {dashboard.paired_devices.map((peer) => (
              <option key={peer.device.id} value={peer.device.id}>
                {peer.device.name} · {deviceStateLabel(locale, peer.state)}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className={`drop-zone ${dropActive ? 'drop-zone-active' : ''}`}>
        <strong>{t(locale, 'dropTitle')}</strong>
        <span>{target?.state === 'connected' ? t(locale, 'selectFiles') : t(locale, 'dropHint')}</span>
        <button onClick={() => void chooseFiles()} disabled={!targetId || target?.state !== 'connected'}>
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
            <button onClick={() => void acceptIncoming()}>{t(locale, 'accept')}</button>
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
    setEvents((current) => [event, ...current.filter((item) => item.transfer_id !== event.transfer_id || item.type !== event.type)].slice(0, 12))
  }

  useEffect(() => {
    void refresh()
    const timer = window.setInterval(() => void refresh(), 2_000)
    return () => window.clearInterval(timer)
  }, [])

  if (!dashboard) {
    return <main className="shell loading-state">{t(locale, 'appName')}</main>
  }

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">{t(locale, 'localDevice')}</p>
          <h1>{t(locale, 'appName')}</h1>
          <p className="device-caption">{dashboard.settings.local_device.name}</p>
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

      {error && <section className="error">{error}</section>}
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
            <button onClick={() => void beginPairing()}>{t(locale, 'startPairing')}</button>
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
