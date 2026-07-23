mod clipboard;
mod commands;
mod discovery;
mod domain;
mod error;
mod file_transfer;
mod pairing;
mod protocol;
mod registry;
mod settings;
mod transport;

use clipboard::ClipboardService;
use commands::{
    accept_file_transfer, cancel_file_transfer, cancel_pairing, clear_pairing,
    get_autostart_enabled, get_dashboard_state, request_pairing, set_autostart_enabled,
    set_default_file_target, set_receive_clipboard, set_ui_locale, start_file_transfer,
    start_pairing, AppState,
};
use file_transfer::FileTransferService;
use pairing::PairingRuntime;
use registry::PeerRegistry;
use settings::SettingsStore;
use std::sync::{Arc, Mutex};
use tauri::{
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};
use transport::{TransportEvent, TransportMessage, TransportRuntime};

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
        .plugin(tauri_plugin_dialog::init())
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
            let (transport_runtime, mut transport_events) =
                TransportRuntime::new(discovery_device.clone(), registry.clone());
            let transport = Arc::new(transport_runtime);
            let clipboard = ClipboardService::new(
                discovery_device.clone(),
                settings.clone(),
                transport.clone(),
            );
            let (file_transfer_runtime, mut transfer_events) =
                FileTransferService::new(transport.clone());
            let transfers = Arc::new(file_transfer_runtime);
            let pairing = Arc::new(PairingRuntime::new(
                discovery_device.clone(),
                settings.clone(),
                settings_store.clone(),
                registry.clone(),
                active_pairing.clone(),
            ));

            app.manage(AppState {
                settings_store: settings_store.clone(),
                settings: settings.clone(),
                registry: registry.clone(),
                active_pairing: active_pairing.clone(),
                pairing: pairing.clone(),
                transport: transport.clone(),
                transfers: transfers.clone(),
            });

            tauri::async_runtime::spawn(async move {
                if let Err(err) = discovery::announce_loop(discovery_device, discovery_port).await {
                    tracing::error!(?err, "LAN discovery announcer stopped");
                }
            });
            let receive_pairing = pairing.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) =
                    discovery::receive_loop_with_pairing(discovery_port, receive_pairing).await
                {
                    tracing::error!(?err, "LAN discovery receiver stopped");
                }
            });

            let listen_transport = (*transport).clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = listen_transport.listen_loop(discovery_port).await {
                    tracing::error!(?err, "TCP transport listener stopped");
                }
            });

            let maintain_transport = (*transport).clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = maintain_transport.maintain_connections().await {
                    tracing::error!(?err, "TCP reconnect loop stopped");
                }
            });

            let clipboard_loop = clipboard.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = clipboard_loop.run().await {
                    tracing::error!(?err, "clipboard watcher stopped");
                }
            });

            let clipboard_events = clipboard.clone();
            let file_events = transfers.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(event) = transport_events.recv().await {
                    match event {
                        TransportEvent::PeerConnected(peer) => {
                            tracing::debug!(device_id = ?peer.id, "peer transport connected");
                        }
                        TransportEvent::PeerDisconnected { peer, reason_code } => {
                            tracing::debug!(device_id = ?peer.id, %reason_code, "peer transport disconnected");
                        }
                        TransportEvent::Message { peer, message } => match message {
                            TransportMessage::Clipboard(clipboard_event) => {
                                if let Err(err) =
                                    clipboard_events.handle_remote(&peer.id, clipboard_event)
                                {
                                    tracing::debug!(?err, device_id = ?peer.id, "clipboard event was rejected");
                                }
                            }
                            message @ (TransportMessage::FileOffer(_)
                            | TransportMessage::FileAccept(_)
                            | TransportMessage::FileChunk(_)
                            | TransportMessage::FileComplete(_)
                            | TransportMessage::FileCancel(_)) => {
                                if let Err(err) = file_events.handle_message(&peer, message).await {
                                    tracing::debug!(?err, device_id = ?peer.id, "file transfer message was rejected");
                                }
                            }
                            _ => {}
                        },
                    }
                }
            });

            let transfer_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(event) = transfer_events.recv().await {
                    if let Err(err) = transfer_app.emit("transfer-event", &event) {
                        tracing::debug!(?err, "failed to emit transfer event");
                    }
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
            start_file_transfer,
            accept_file_transfer,
            cancel_file_transfer,
            get_autostart_enabled,
            set_autostart_enabled,
            start_pairing,
            cancel_pairing,
            request_pairing,
            set_receive_clipboard,
            set_default_file_target,
            set_ui_locale,
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
