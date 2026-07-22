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
}

export interface DashboardState {
  settings: LocalSettings
  discovered_devices: DeviceInfo[]
  paired_devices: PairedPeer[]
  active_pairing_code: string | null
}
