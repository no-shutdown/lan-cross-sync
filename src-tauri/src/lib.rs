mod commands;
mod discovery;
mod domain;
mod error;
mod pairing;
mod protocol;
mod registry;
mod settings;

use commands::{
    cancel_pairing, clear_pairing, get_autostart_enabled, get_dashboard_state,
    set_autostart_enabled, set_default_file_target, set_receive_clipboard, start_pairing, AppState,
};
use registry::PeerRegistry;
use settings::SettingsStore;
use std::sync::{Arc, Mutex};
use tauri::{
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "lan-cross-sync";
const TRAY_SHOW_ID: &str = "show";
const TRAY_QUIT_ID: &str = "quit";

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
            let discovery_device = settings.local_device.clone();
            let discovery_port = discovery_device.port;
            let settings = Arc::new(Mutex::new(settings));
            let registry = Arc::new(Mutex::new(registry));
            let active_pairing = Arc::new(Mutex::new(None));

            app.manage(AppState {
                settings_store,
                settings: settings.clone(),
                registry: registry.clone(),
                active_pairing,
            });

            tauri::async_runtime::spawn(async move {
                if let Err(err) = discovery::announce_loop(discovery_device, discovery_port).await {
                    tracing::error!(?err, "LAN discovery announcer stopped");
                }
            });
            let receive_device_id = settings
                .lock()
                .expect("settings lock poisoned during startup")
                .local_device
                .id
                .clone();
            let receive_registry = registry.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) =
                    discovery::receive_loop(receive_device_id, discovery_port, receive_registry)
                        .await
                {
                    tracing::error!(?err, "LAN discovery receiver stopped");
                }
            });

            setup_tray(app)?;
            setup_close_to_tray(app);

            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id() == TRAY_SHOW_ID {
                show_main_window(app);
            } else if event.id() == TRAY_QUIT_ID {
                app.exit(0);
            }
        })
        .on_tray_icon_event(|app, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(app);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard_state,
            get_autostart_enabled,
            set_autostart_enabled,
            start_pairing,
            cancel_pairing,
            set_receive_clipboard,
            set_default_file_target,
            clear_pairing
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(TRAY_SHOW_ID, "Show LAN Cross Sync")
        .separator()
        .text(TRAY_QUIT_ID, "Quit")
        .build()?;
    let icon = app
        .default_window_icon()
        .cloned()
        .expect("missing bundled app icon");

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("LAN Cross Sync")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .build(app)?;

    Ok(())
}

fn setup_close_to_tray(app: &tauri::App) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let window_to_hide = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(err) = window_to_hide.hide() {
                    tracing::error!(?err, "failed to hide main window");
                }
            }
        });
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Err(err) = window.show() {
            tracing::error!(?err, "failed to show main window");
        }
        if let Err(err) = window.set_focus() {
            tracing::error!(?err, "failed to focus main window");
        }
    }
}
