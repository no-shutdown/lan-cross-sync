import { invoke } from '@tauri-apps/api/core'
import type { DashboardState, DeviceId, LocalSettings, Locale } from './types'

export function getDashboardState(): Promise<DashboardState> {
  return invoke('get_dashboard_state')
}

export function getAutostartEnabled(): Promise<boolean> {
  return invoke('get_autostart_enabled')
}

export function setAutostartEnabled(enabled: boolean): Promise<boolean> {
  return invoke('set_autostart_enabled', { enabled })
}

export function startPairing(): Promise<string> {
  return invoke('start_pairing')
}

export function cancelPairing(): Promise<void> {
  return invoke('cancel_pairing')
}

export function requestPairing(deviceId: DeviceId, code: string): Promise<string> {
  return invoke('request_pairing', { deviceId, code })
}

export function setUiLocale(locale: Locale): Promise<LocalSettings> {
  return invoke('set_ui_locale', { locale })
}

export function setDeviceName(name: string): Promise<LocalSettings> {
  return invoke('set_device_name', { name })
}

export function startFileTransfer(deviceId: DeviceId, paths: string[]): Promise<string> {
  return invoke('start_file_transfer', { deviceId, paths })
}

export function acceptFileTransfer(transferId: string, destination: string): Promise<void> {
  return invoke('accept_file_transfer', { transferId, destination })
}

export function cancelFileTransfer(transferId: string): Promise<void> {
  return invoke('cancel_file_transfer', { transferId })
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
