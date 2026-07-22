import { useEffect, useState } from 'react'
import './App.css'
import {
  cancelPairing,
  clearPairing,
  getDashboardState,
  setDefaultFileTarget,
  setReceiveClipboard,
  startPairing,
} from './lib/api'
import type { DashboardState, DeviceId, PairedPeer } from './lib/types'

function deviceIdText(id: DeviceId): string {
  return id
}

function PeerCard({ peer, refresh }: { peer: PairedPeer; refresh: () => Promise<void> }) {
  async function toggleClipboard() {
    await setReceiveClipboard(peer.device.id, !peer.receive_clipboard)
    await refresh()
  }

  async function makeDefaultTarget() {
    await setDefaultFileTarget(peer.device.id)
    await refresh()
  }

  async function removePairing() {
    await clearPairing(peer.device.id)
    await refresh()
  }

  return (
    <article className="peer-card">
      <div>
        <h3>{peer.device.name}</h3>
        <p>{peer.state}</p>
        <code>{deviceIdText(peer.device.id)}</code>
      </div>
      <div className="peer-actions">
        <label>
          <input type="checkbox" checked={peer.receive_clipboard} onChange={toggleClipboard} />
          Receive clipboard
        </label>
        <button onClick={makeDefaultTarget} disabled={peer.is_default_file_target}>
          {peer.is_default_file_target ? 'Default target' : 'Set file target'}
        </button>
        <button className="danger" onClick={removePairing}>
          Clear pairing
        </button>
      </div>
    </article>
  )
}

export default function App() {
  const [dashboard, setDashboard] = useState<DashboardState | null>(null)
  const [error, setError] = useState<string | null>(null)

  async function refresh() {
    try {
      setDashboard(await getDashboardState())
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load dashboard state.')
    }
  }

  async function beginPairing() {
    try {
      await startPairing()
      await refresh()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to start pairing.')
    }
  }

  async function stopPairing() {
    await cancelPairing()
    await refresh()
  }

  useEffect(() => {
    void refresh()
    const timer = window.setInterval(() => void refresh(), 2_000)
    return () => window.clearInterval(timer)
  }, [])

  if (!dashboard) {
    return <main className="shell">Loading...</main>
  }

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <h1>LAN Cross Sync</h1>
          <p>{dashboard.settings.local_device.name}</p>
        </div>
        <button onClick={() => void refresh()}>Refresh</button>
      </header>

      {error && <section className="error">{error}</section>}

      <section className="panel">
        <div className="panel-title">
          <h2>Pairing</h2>
          {dashboard.active_pairing_code ? (
            <button onClick={() => void stopPairing()}>Cancel</button>
          ) : (
            <button onClick={() => void beginPairing()}>Start pairing</button>
          )}
        </div>
        {dashboard.active_pairing_code ? (
          <div className="pairing-code">{dashboard.active_pairing_code}</div>
        ) : (
          <p>No active pairing session.</p>
        )}
      </section>

      <section className="panel">
        <h2>Paired devices</h2>
        {dashboard.paired_devices.length === 0 ? (
          <p>No paired devices yet.</p>
        ) : (
          <div className="peer-list">
            {dashboard.paired_devices.map((peer) => (
              <PeerCard key={deviceIdText(peer.device.id)} peer={peer} refresh={refresh} />
            ))}
          </div>
        )}
      </section>

      <section className="panel">
        <h2>Discovered devices</h2>
        {dashboard.discovered_devices.length === 0 ? (
          <p>No unpaired devices discovered.</p>
        ) : (
          <div className="peer-list">
            {dashboard.discovered_devices.map((device) => (
              <article className="peer-card" key={deviceIdText(device.id)}>
                <div>
                  <h3>{device.name}</h3>
                  <p>Available to pair</p>
                  <code>{deviceIdText(device.id)}</code>
                </div>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="drop-zone" aria-label="Future file drop zone">
        <strong>File drop area</strong>
        <span>Foundation only. File transfer comes in the next implementation plan.</span>
      </section>
    </main>
  )
}
