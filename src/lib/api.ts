import { invoke } from '@tauri-apps/api/core'
import type { DashboardState, DeviceId, LocalSettings } from './types'

export function getDashboardState(): Promise<DashboardState> {
  return invoke('get_dashboard_state')
}

export function startPairing(): Promise<string> {
  return invoke('start_pairing')
}

export function cancelPairing(): Promise<void> {
  return invoke('cancel_pairing')
}

export function setReceiveClipboard(deviceId: DeviceId, enabled: boolean): Promise<LocalSettings> {
  return invoke('set_receive_clipboard', { deviceId, enabled })
}

export function setDefaultFileTarget(deviceId: DeviceId): Promise<LocalSettings> {
  return invoke('set_default_file_target', { deviceId })
}

export function clearPairing(deviceId: DeviceId): Promise<LocalSettings> {
  return invoke('clear_pairing', { deviceId })
}
