use crate::{
    domain::{DeviceId, LocalSettings, PairedPeer},
    error::{AppError, AppResult},
    pairing::PairingSession,
    registry::PeerRegistry,
    settings::SettingsStore,
};
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri_plugin_autostart::ManagerExt;

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
    let mut settings = state.settings.lock().unwrap().clone();
    let registry = state.registry.lock().unwrap();
    let mut active_pairing = state.active_pairing.lock().unwrap();
    let active_pairing_code = active_pairing_code(&mut active_pairing);
    let paired_devices = registry.paired();
    settings.paired_peers = paired_devices.clone();

    Ok(DashboardState {
        settings,
        paired_devices,
        discovered_devices: registry.discovered(),
        active_pairing_code,
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
    state.registry.lock().unwrap().remove_pairing(&device_id);
    Ok(next)
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
    fn active_pairing_code_clears_expired_session() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let mut active_pairing = Some(PairingSession::expired_for_test(device, "123456"));

        let code = active_pairing_code(&mut active_pairing);

        assert_eq!(code, None);
        assert!(active_pairing.is_none());
    }

    #[test]
    fn active_pairing_code_returns_unexpired_code() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let mut active_pairing = Some(PairingSession::with_code_for_test(device, "123456"));

        let code = active_pairing_code(&mut active_pairing);

        assert_eq!(code, Some("123456".to_string()));
        assert!(active_pairing.is_some());
    }
}
