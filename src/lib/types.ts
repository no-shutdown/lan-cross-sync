export type DeviceId = string

export interface DeviceInfo {
  id: DeviceId
  name: string
  app_version: string
  protocol_version: number
  port: number
  capabilities: string[]
}

export type PeerConnectionState = 'offline' | 'discovered' | 'pairing' | 'connected' | 'error'

export interface PairedPeer {
  device: DeviceInfo
  receive_clipboard: boolean
  is_default_file_target: boolean
  state: PeerConnectionState
}

export interface LocalSettings {
  local_device: DeviceInfo
  paired_peers: PairedPeer[]
  ui_locale: Locale
}

export type Locale = 'zh-CN' | 'en-US'

export interface NetworkStatus {
  discovery_port: number
  transport_port: number | null
  discovery_ready: boolean
  transport_ready: boolean
  advertising: boolean
  issue_code: string | null
}

export interface DashboardState {
  settings: LocalSettings
  discovered_devices: DeviceInfo[]
  paired_devices: PairedPeer[]
  active_pairing_code: string | null
  pairing_error_code: string | null
  network_status: NetworkStatus
}

export interface ManifestEntry {
  relative_path: string
  kind: 'File' | 'Directory'
  size: number
}

export interface TransferManifest {
  root_name: string
  total_bytes: number
  entries: ManifestEntry[]
}

export type TransferDirection = 'sending' | 'receiving'

export type TransferEvent =
  | {
      type: 'offer'
      transfer_id: string
      peer: DeviceInfo
      manifest: TransferManifest
      direction: TransferDirection
    }
  | {
      type: 'progress'
      transfer_id: string
      peer_id: DeviceId
      direction: TransferDirection
      transferred_bytes: number
      total_bytes: number
    }
  | {
      type: 'completed'
      transfer_id: string
      peer_id: DeviceId
      direction: TransferDirection
    }
  | {
      type: 'failed'
      transfer_id: string
      peer_id: DeviceId
      direction: TransferDirection
      reason_code: string
    }
  | {
      type: 'cancelled'
      transfer_id: string
      peer_id: DeviceId
      direction: TransferDirection
    }
