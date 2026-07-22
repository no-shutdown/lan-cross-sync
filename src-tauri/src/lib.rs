mod commands;
mod discovery;
mod domain;
mod error;
mod pairing;
mod protocol;
mod registry;
mod settings;

use commands::{
    cancel_pairing, clear_pairing, get_dashboard_state, set_default_file_target,
    set_receive_clipboard, start_pairing, AppState,
};
use registry::PeerRegistry;
use settings::SettingsStore;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let builder = builder.plugin(tauri_plugin_autostart::Builder::new().build());

    builder
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_config = app
                .path()
                .app_config_dir()
                .expect("failed to resolve app config directory");
            let settings_store = SettingsStore::new(app_config.join("settings.json"));
            let settings = settings_store
                .load_or_create("LAN Cross Sync")
                .expect("failed to load settings");
            let registry = PeerRegistry::from_paired(settings.paired_peers.clone());

            app.manage(AppState {
                settings_store,
                settings: Mutex::new(settings),
                registry: Mutex::new(registry),
                active_pairing: Mutex::new(None),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard_state,
            start_pairing,
            cancel_pairing,
            set_receive_clipboard,
            set_default_file_target,
            clear_pairing
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
