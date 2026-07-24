use crate::{
    domain::{DeviceId, LocalSettings, PairedPeer},
    error::{AppError, AppResult},
    file_transfer::FileTransferService,
    pairing::{self, PairingRuntime, PairingSession},
    registry::PeerRegistry,
    settings::{SettingsStore, DEFAULT_DISCOVERY_PORT},
    transport::TransportRuntime,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::State;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri_plugin_autostart::ManagerExt;
use tokio::net::UdpSocket;

pub struct AppState {
    pub settings_store: SettingsStore,
    pub settings: Arc<Mutex<LocalSettings>>,
    pub registry: Arc<Mutex<PeerRegistry>>,
    pub active_pairing: Arc<Mutex<Option<PairingSession>>>,
    pub pairing: Arc<PairingRuntime>,
    pub transport: Arc<TransportRuntime>,
    pub transfers: Arc<FileTransferService>,
    pub network_status: Arc<Mutex<NetworkStatus>>,
    pub discovery_socket: Option<Arc<UdpSocket>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NetworkStatus {
    pub discovery_port: u16,
    pub transport_port: Option<u16>,
    pub discovery_ready: bool,
    pub transport_ready: bool,
    pub advertising: bool,
    pub issue_code: Option<String>,
}

impl NetworkStatus {
    pub fn from_bindings(
        discovery_ready: bool,
        transport_port: Option<u16>,
        transport_fallback: bool,
    ) -> Self {
        let transport_ready = transport_port.is_some();
        let advertising = discovery_ready && transport_ready;
        let issue_code = if !discovery_ready && !transport_ready {
            Some("network_services_unavailable".to_string())
        } else if !discovery_ready {
            Some("network_discovery_unavailable".to_string())
        } else if !transport_ready {
            Some("network_transport_unavailable".to_string())
        } else if transport_fallback {
            Some("transport_port_fallback".to_string())
        } else {
            None
        };

        Self {
            discovery_port: DEFAULT_DISCOVERY_PORT,
            transport_port,
            discovery_ready,
            transport_ready,
            advertising,
            issue_code,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DashboardState {
    pub settings: LocalSettings,
    pub discovered_devices: Vec<crate::domain::DeviceInfo>,
    pub paired_devices: Vec<PairedPeer>,
    pub active_pairing_code: Option<String>,
    pub pairing_error_code: Option<String>,
    pub network_status: NetworkStatus,
}

#[tauri::command]
pub fn get_dashboard_state(state: State<'_, AppState>) -> AppResult<DashboardState> {
    let mut settings = state.settings.lock().unwrap().clone();
    let registry = state.registry.lock().unwrap();
    let mut active_pairing = state.active_pairing.lock().unwrap();
    let active_pairing_code = active_pairing_code(&mut active_pairing);
    let paired_devices = registry.paired();
    settings.paired_peers = paired_devices.clone();
    let pairing_error_code = state.pairing.last_error.lock().unwrap().clone();
    let network_status = state.network_status.lock().unwrap().clone();

    Ok(DashboardState {
        settings,
        paired_devices,
        discovered_devices: registry.discovered(),
        active_pairing_code,
        pairing_error_code,
        network_status,
    })
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
pub fn get_autostart_enabled(app: tauri::AppHandle) -> AppResult<bool> {
    app.autolaunch()
        .is_enabled()
        .map_err(|err| AppError::Message(format!("Failed to read autostart state: {err}")))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
#[tauri::command]
pub fn get_autostart_enabled(_app: tauri::AppHandle) -> AppResult<bool> {
    Err(AppError::Message(
        "Autostart is only available on desktop.".to_string(),
    ))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
pub fn set_autostart_enabled(app: tauri::AppHandle, enabled: bool) -> AppResult<bool> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };

    result.map_err(|err| AppError::Message(format!("Failed to update autostart: {err}")))?;
    manager
        .is_enabled()
        .map_err(|err| AppError::Message(format!("Failed to read autostart state: {err}")))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
#[tauri::command]
pub fn set_autostart_enabled(_app: tauri::AppHandle, _enabled: bool) -> AppResult<bool> {
    Err(AppError::Message(
        "Autostart is only available on desktop.".to_string(),
    ))
}

#[tauri::command]
pub fn start_pairing(state: State<'_, AppState>) -> AppResult<String> {
    let session = PairingSession::new();
    let code = session.code.clone();
    *state.active_pairing.lock().unwrap() = Some(session);
    state.pairing.clear_error();
    Ok(code)
}

#[tauri::command]
pub fn cancel_pairing(state: State<'_, AppState>) -> AppResult<()> {
    *state.active_pairing.lock().unwrap() = None;
    Ok(())
}

#[tauri::command]
pub async fn request_pairing(
    state: State<'_, AppState>,
    device_id: DeviceId,
    code: String,
) -> AppResult<String> {
    if code.len() != 6 || !code.chars().all(|character| character.is_ascii_digit()) {
        return Err(AppError::Message("invalid_pairing_code".to_string()));
    }

    let (target, endpoint) = {
        let registry = state.registry.lock().unwrap();
        let target = registry
            .device(&device_id)
            .ok_or_else(|| AppError::Message("device_not_found".to_string()))?;
        let endpoint = registry
            .discovery_endpoint(&device_id)
            .ok_or_else(|| AppError::Message("device_endpoint_unavailable".to_string()))?;
        (target, endpoint)
    };
    let socket = state
        .discovery_socket
        .as_ref()
        .ok_or_else(|| AppError::Message("network_discovery_unavailable".to_string()))?;

    state.pairing.clear_error();

    // Sending from the shared discovery socket (rather than a throwaway one)
    // is required: the peer replies to this packet's source port, and only
    // the shared socket is polled by the receive loop for that reply.
    pairing::send_pairing_request(socket, &state.pairing, target, endpoint, code)
        .await
        .map_err(AppError::Anyhow)
}

#[tauri::command]
pub async fn start_file_transfer(
    state: State<'_, AppState>,
    device_id: DeviceId,
    paths: Vec<String>,
) -> AppResult<String> {
    let paths = paths.into_iter().map(PathBuf::from).collect();
    state
        .transfers
        .start_transfer(device_id, paths)
        .await
        .map_err(AppError::Anyhow)
}

#[tauri::command]
pub async fn accept_file_transfer(
    state: State<'_, AppState>,
    transfer_id: String,
    destination: String,
) -> AppResult<()> {
    state
        .transfers
        .accept_transfer(&transfer_id, PathBuf::from(destination))
        .await
        .map_err(AppError::Anyhow)
}

#[tauri::command]
pub async fn cancel_file_transfer(
    state: State<'_, AppState>,
    transfer_id: String,
) -> AppResult<()> {
    state
        .transfers
        .cancel_transfer(&transfer_id)
        .await
        .map_err(AppError::Anyhow)
}

#[tauri::command]
pub fn set_device_name(state: State<'_, AppState>, name: String) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    let next = with_device_name(settings.clone(), name)?;
    state.settings_store.save(&next)?;
    *settings = next.clone();
    Ok(next)
}

#[tauri::command]
pub fn set_ui_locale(state: State<'_, AppState>, locale: String) -> AppResult<LocalSettings> {
    if !matches!(locale.as_str(), "zh-CN" | "en-US") {
        return Err(AppError::Message("invalid_locale".to_string()));
    }
    let mut settings = state.settings.lock().unwrap();
    let mut next = settings.clone();
    next.ui_locale = locale;
    state.settings_store.save(&next)?;
    *settings = next.clone();
    Ok(next)
}

#[tauri::command]
pub fn set_receive_clipboard(
    state: State<'_, AppState>,
    device_id: DeviceId,
    enabled: bool,
) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    let next = with_receive_clipboard(settings.clone(), device_id, enabled)?;
    state.settings_store.save(&next)?;
    *settings = next.clone();
    state
        .registry
        .lock()
        .unwrap()
        .sync_preferences(&next.paired_peers);
    Ok(next)
}

#[tauri::command]
pub fn set_default_file_target(
    state: State<'_, AppState>,
    device_id: DeviceId,
) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    let next = with_default_file_target(settings.clone(), device_id)?;
    state.settings_store.save(&next)?;
    *settings = next.clone();
    state
        .registry
        .lock()
        .unwrap()
        .sync_preferences(&next.paired_peers);
    Ok(next)
}

#[tauri::command]
pub fn clear_pairing(state: State<'_, AppState>, device_id: DeviceId) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    let next = without_pairing(settings.clone(), device_id.clone())?;
    state.settings_store.save(&next)?;
    *settings = next.clone();
    state.transport.disconnect_peer(&device_id);
    state.registry.lock().unwrap().remove_pairing(&device_id);
    Ok(next)
}

fn with_device_name(mut settings: LocalSettings, name: String) -> AppResult<LocalSettings> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 40 {
        return Err(AppError::Message("invalid_device_name".to_string()));
    }
    settings.local_device.name = trimmed.to_string();
    Ok(settings)
}

fn with_receive_clipboard(
    mut settings: LocalSettings,
    device_id: DeviceId,
    enabled: bool,
) -> AppResult<LocalSettings> {
    let peer = settings
        .paired_peers
        .iter_mut()
        .find(|peer| peer.device.id == device_id)
        .ok_or_else(|| AppError::Message("Paired device not found.".to_string()))?;
    peer.receive_clipboard = enabled;
    Ok(settings)
}

fn with_default_file_target(
    mut settings: LocalSettings,
    device_id: DeviceId,
) -> AppResult<LocalSettings> {
    if !settings
        .paired_peers
        .iter()
        .any(|peer| peer.device.id == device_id)
    {
        return Err(AppError::Message("Paired device not found.".to_string()));
    }

    for peer in &mut settings.paired_peers {
        peer.is_default_file_target = peer.device.id == device_id;
    }

    Ok(settings)
}

fn without_pairing(mut settings: LocalSettings, device_id: DeviceId) -> AppResult<LocalSettings> {
    let previous_len = settings.paired_peers.len();
    settings
        .paired_peers
        .retain(|peer| peer.device.id != device_id);
    if settings.paired_peers.len() == previous_len {
        return Err(AppError::Message("Paired device not found.".to_string()));
    }
    Ok(settings)
}

fn active_pairing_code(active_pairing: &mut Option<PairingSession>) -> Option<String> {
    if active_pairing
        .as_ref()
        .is_some_and(PairingSession::is_expired)
    {
        *active_pairing = None;
        return None;
    }

    active_pairing.as_ref().map(|session| session.code.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DeviceInfo, PeerConnectionState};

    fn local_settings(peers: Vec<PairedPeer>) -> LocalSettings {
        LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: peers,
            ui_locale: "zh-CN".to_string(),
        }
    }

    fn paired_peer(name: &str, is_default_file_target: bool) -> PairedPeer {
        PairedPeer {
            device: DeviceInfo::new_local(name, 45731),
            receive_clipboard: true,
            is_default_file_target,
            state: PeerConnectionState::Connected,
        }
    }

    #[test]
    fn unknown_default_target_preserves_existing_default() {
        let first = paired_peer("MacBook", true);
        let second = paired_peer("Linux Desk", false);
        let settings = local_settings(vec![first.clone(), second.clone()]);

        let result = with_default_file_target(settings.clone(), DeviceId::new());

        assert!(matches!(result, Err(AppError::Message(_))));
        assert!(settings.paired_peers[0].is_default_file_target);
        assert!(!settings.paired_peers[1].is_default_file_target);
    }

    #[test]
    fn default_target_update_makes_exactly_one_peer_default() {
        let first = paired_peer("MacBook", true);
        let second = paired_peer("Linux Desk", false);
        let target_id = second.device.id.clone();
        let settings = local_settings(vec![first, second]);

        let updated = with_default_file_target(settings, target_id.clone()).unwrap();

        assert_eq!(
            updated
                .paired_peers
                .iter()
                .filter(|peer| peer.is_default_file_target)
                .count(),
            1
        );
        assert!(updated
            .paired_peers
            .iter()
            .any(|peer| peer.device.id == target_id && peer.is_default_file_target));
    }

    #[test]
    fn clear_pairing_removes_peer() {
        let first = paired_peer("MacBook", true);
        let second = paired_peer("Linux Desk", false);
        let removed_id = first.device.id.clone();
        let settings = local_settings(vec![first, second.clone()]);

        let updated = without_pairing(settings, removed_id.clone()).unwrap();

        assert_eq!(updated.paired_peers, vec![second]);
        assert!(!updated
            .paired_peers
            .iter()
            .any(|peer| peer.device.id == removed_id));
    }

    #[test]
    fn clear_pairing_errors_on_unknown_id() {
        let settings = local_settings(vec![paired_peer("MacBook", true)]);

        let result = without_pairing(settings, DeviceId::new());

        assert!(matches!(result, Err(AppError::Message(_))));
    }

    #[test]
    fn device_name_update_trims_and_applies_new_name() {
        let settings = local_settings(vec![]);

        let updated = with_device_name(settings, "  Study PC  ".to_string()).unwrap();

        assert_eq!(updated.local_device.name, "Study PC");
    }

    #[test]
    fn device_name_update_rejects_blank_name() {
        let settings = local_settings(vec![]);

        let result = with_device_name(settings, "   ".to_string());

        assert!(matches!(result, Err(AppError::Message(_))));
    }

    #[test]
    fn device_name_update_rejects_overly_long_name() {
        let settings = local_settings(vec![]);

        let result = with_device_name(settings, "x".repeat(41));

        assert!(matches!(result, Err(AppError::Message(_))));
    }

    #[test]
    fn active_pairing_code_clears_expired_session() {
        let mut active_pairing = Some(PairingSession::expired_for_test("123456"));

        let code = active_pairing_code(&mut active_pairing);

        assert_eq!(code, None);
        assert!(active_pairing.is_none());
    }

    #[test]
    fn active_pairing_code_returns_unexpired_code() {
        let mut active_pairing = Some(PairingSession::with_code_for_test("123456"));

        let code = active_pairing_code(&mut active_pairing);

        assert_eq!(code, Some("123456".to_string()));
        assert!(active_pairing.is_some());
    }

    #[test]
    fn network_status_reports_fallback_and_binding_failures() {
        let status = NetworkStatus::from_bindings(true, Some(46001), true);
        let json = serde_json::to_value(status).unwrap();

        assert_eq!(json["discovery_port"], DEFAULT_DISCOVERY_PORT);
        assert_eq!(json["transport_port"], 46001);
        assert_eq!(json["discovery_ready"], true);
        assert_eq!(json["transport_ready"], true);
        assert_eq!(json["advertising"], true);
        assert_eq!(json["issue_code"], "transport_port_fallback");

        let unavailable = NetworkStatus::from_bindings(false, None, false);
        let unavailable_json = serde_json::to_value(unavailable).unwrap();
        assert_eq!(unavailable_json["advertising"], false);
        assert_eq!(
            unavailable_json["issue_code"],
            "network_services_unavailable"
        );
    }
}
