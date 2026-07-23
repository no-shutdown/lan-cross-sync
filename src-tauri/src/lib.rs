#![allow(linker_messages)]

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
    start_pairing, AppState, NetworkStatus,
};
use file_transfer::FileTransferService;
use pairing::PairingRuntime;
use registry::PeerRegistry;
use settings::{SettingsStore, DEFAULT_DISCOVERY_PORT};
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
            let mut loaded_settings = settings_store
                .load_or_create("LAN Cross Sync")
                .expect("failed to load settings");
            let preferred_transport_port = loaded_settings.local_device.port;
            let (transport_listener, transport_port, transport_fallback) =
                match tauri::async_runtime::block_on(transport::bind_transport_listener(
                    preferred_transport_port,
                )) {
                    Ok((listener, port, used_fallback)) => {
                        (Some(listener), Some(port), used_fallback)
                    }
                    Err(err) => {
                        tracing::error!(
                            ?err,
                            preferred_transport_port,
                            "TCP transport listener could not be bound"
                        );
                        (None, None, false)
                    }
                };
            let discovery_socket = match tauri::async_runtime::block_on(
                discovery::bind_discovery_socket(DEFAULT_DISCOVERY_PORT),
            ) {
                Ok(socket) => Some(socket),
                Err(err) => {
                    tracing::error!(?err, "LAN discovery UDP listener could not be bound");
                    None
                }
            };

            let discovery_ready = discovery_socket.is_some();
            let transport_ready = transport_listener.is_some();
            let network_status = Arc::new(Mutex::new(NetworkStatus::from_bindings(
                discovery_ready,
                transport_port,
                transport_fallback,
            )));
            let advertising = network_status.lock().unwrap().advertising;

            if let Some(actual_port) = transport_port {
                if loaded_settings.local_device.port != actual_port {
                    loaded_settings.local_device.port = actual_port;
                    if let Err(err) = settings_store.save(&loaded_settings) {
                        tracing::error!(?err, "failed to persist the active TCP transport port");
                    }
                }
            }

            let registry = PeerRegistry::from_paired(loaded_settings.paired_peers.clone());
            let discovery_device = loaded_settings.local_device.clone();
            let settings = Arc::new(Mutex::new(loaded_settings));
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
            let transfer_index_path = app
                .path()
                .app_cache_dir()
                .expect("failed to resolve application cache directory")
                .join("active-transfers.json");
            let (file_transfer_runtime, mut transfer_events) =
                FileTransferService::new(transport.clone(), transfer_index_path)
                    .expect("failed to initialize file transfer staging");
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
                network_status: network_status.clone(),
            });

            if advertising {
                let announce_device = discovery_device.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) =
                        discovery::announce_loop(announce_device, DEFAULT_DISCOVERY_PORT).await
                    {
                        tracing::error!(?err, "LAN discovery announcer stopped");
                    }
                });
            }

            if let Some(discovery_socket) = discovery_socket {
                let receive_pairing = pairing.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) =
                        discovery::receive_loop_with_pairing(discovery_socket, receive_pairing)
                            .await
                    {
                        tracing::error!(?err, "LAN discovery receiver stopped");
                    }
                });
            }

            if let Some(transport_listener) = transport_listener {
                let listen_transport = (*transport).clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = listen_transport.listen_loop(transport_listener).await {
                        tracing::error!(?err, "TCP transport listener stopped");
                    }
                });
            }

            if transport_ready {
                let maintain_transport = (*transport).clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = maintain_transport.maintain_connections().await {
                        tracing::error!(?err, "TCP reconnect loop stopped");
                    }
                });
            }

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
                            file_events.handle_peer_disconnected(&peer.id);
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
