use crate::{
    domain::{DeviceId, LocalSettings, PairedPeer, PeerConnectionState},
    error::{AppError, AppResult},
    pairing::PairingSession,
    registry::PeerRegistry,
    settings::SettingsStore,
};
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub settings_store: SettingsStore,
    pub settings: Mutex<LocalSettings>,
    pub registry: Mutex<PeerRegistry>,
    pub active_pairing: Mutex<Option<PairingSession>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DashboardState {
    pub settings: LocalSettings,
    pub discovered_devices: Vec<crate::domain::DeviceInfo>,
    pub paired_devices: Vec<PairedPeer>,
    pub active_pairing_code: Option<String>,
}

#[tauri::command]
pub fn get_dashboard_state(state: State<'_, AppState>) -> AppResult<DashboardState> {
    let settings = state.settings.lock().unwrap().clone();
    let registry = state.registry.lock().unwrap();
    let active_pairing_code = state
        .active_pairing
        .lock()
        .unwrap()
        .as_ref()
        .map(|session| session.code.clone());

    Ok(DashboardState {
        paired_devices: settings.paired_peers.clone(),
        settings,
        discovered_devices: registry.discovered(),
        active_pairing_code,
    })
}

#[tauri::command]
pub fn start_pairing(state: State<'_, AppState>) -> AppResult<String> {
    let local_device = state.settings.lock().unwrap().local_device.clone();
    let session = PairingSession::new(local_device);
    let code = session.code.clone();
    *state.active_pairing.lock().unwrap() = Some(session);
    Ok(code)
}

#[tauri::command]
pub fn cancel_pairing(state: State<'_, AppState>) -> AppResult<()> {
    *state.active_pairing.lock().unwrap() = None;
    Ok(())
}

#[tauri::command]
pub fn set_receive_clipboard(
    state: State<'_, AppState>,
    device_id: DeviceId,
    enabled: bool,
) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    let peer = settings
        .paired_peers
        .iter_mut()
        .find(|peer| peer.device.id == device_id)
        .ok_or_else(|| AppError::Message("Paired device not found.".to_string()))?;
    peer.receive_clipboard = enabled;
    state.settings_store.save(&settings)?;
    Ok(settings.clone())
}

#[tauri::command]
pub fn set_default_file_target(
    state: State<'_, AppState>,
    device_id: DeviceId,
) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    let mut found = false;
    for peer in &mut settings.paired_peers {
        let is_target = peer.device.id == device_id;
        peer.is_default_file_target = is_target;
        found |= is_target;
    }
    if !found {
        return Err(AppError::Message("Paired device not found.".to_string()));
    }
    state.settings_store.save(&settings)?;
    Ok(settings.clone())
}

#[tauri::command]
pub fn clear_pairing(state: State<'_, AppState>, device_id: DeviceId) -> AppResult<LocalSettings> {
    let mut settings = state.settings.lock().unwrap();
    settings
        .paired_peers
        .retain(|peer| peer.device.id != device_id);
    state.settings_store.save(&settings)?;
    state
        .registry
        .lock()
        .unwrap()
        .set_state(&device_id, PeerConnectionState::Offline);
    Ok(settings.clone())
}
