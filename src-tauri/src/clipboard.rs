use crate::{
    domain::{DeviceId, DeviceInfo, LocalSettings},
    transport::{TransportMessage, TransportRuntime},
};
use arboard::{Clipboard, ImageData};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "windows"))]
use std::time::Duration;
use std::{
    borrow::Cow,
    collections::HashSet,
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
#[cfg(target_os = "windows")]
use tokio::sync::mpsc;
#[cfg(not(target_os = "windows"))]
use tokio::time;
use uuid::Uuid;

#[cfg(not(target_os = "windows"))]
pub const CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ClipboardPayload {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        width: u32,
        height: u32,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClipboardEvent {
    pub event_id: String,
    pub source_device_id: DeviceId,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub content_hash: String,
    pub payload: ClipboardPayload,
}

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("image payload is too large: {0} bytes")]
    ImageTooLarge(usize),
    #[error("image payload has invalid dimensions")]
    InvalidImage,
    #[error("clipboard operation failed: {0}")]
    System(String),
    #[error("clipboard event source is not the connected peer")]
    UnauthorizedSource,
}

impl ClipboardEvent {
    pub fn from_text(
        source_device_id: DeviceId,
        sequence: u64,
        timestamp_ms: u64,
        text: impl Into<String>,
    ) -> Self {
        let text = text.into();
        let content_hash = content_hash(format!("text:{text}").as_bytes());
        Self {
            event_id: Uuid::new_v4().to_string(),
            source_device_id,
            sequence,
            timestamp_ms,
            content_hash,
            payload: ClipboardPayload::Text { text },
        }
    }

    pub fn from_image(
        source_device_id: DeviceId,
        sequence: u64,
        timestamp_ms: u64,
        width: u32,
        height: u32,
        data: Vec<u8>,
    ) -> Result<Self, ClipboardError> {
        validate_image(width, height, &data)?;
        let mut hash_input = Vec::with_capacity(16 + data.len());
        hash_input.extend_from_slice(format!("image:{width}x{height}:").as_bytes());
        hash_input.extend_from_slice(&data);
        Ok(Self {
            event_id: Uuid::new_v4().to_string(),
            source_device_id,
            sequence,
            timestamp_ms,
            content_hash: content_hash(&hash_input),
            payload: ClipboardPayload::Image {
                mime_type: "image/raw-rgba".to_string(),
                width,
                height,
                data,
            },
        })
    }
}

#[derive(Default)]
pub struct ClipboardTracker {
    seen_hashes: HashSet<String>,
}

impl ClipboardTracker {
    pub fn observe_local(&mut self, event: &ClipboardEvent) -> bool {
        self.seen_hashes.insert(event.content_hash.clone())
    }

    pub fn accept_remote(&mut self, event: &ClipboardEvent) -> bool {
        self.seen_hashes.insert(event.content_hash.clone())
    }
}

#[derive(Clone)]
pub struct ClipboardService {
    local_device: DeviceInfo,
    settings: Arc<Mutex<LocalSettings>>,
    transport: Arc<TransportRuntime>,
    tracker: Arc<Mutex<ClipboardTracker>>,
    sequence: Arc<AtomicU64>,
}

impl ClipboardService {
    pub fn new(
        local_device: DeviceInfo,
        settings: Arc<Mutex<LocalSettings>>,
        transport: Arc<TransportRuntime>,
    ) -> Self {
        Self {
            local_device,
            settings,
            transport,
            tracker: Arc::new(Mutex::new(ClipboardTracker::default())),
            sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        #[cfg(target_os = "windows")]
        {
            return self.run_windows().await;
        }

        #[cfg(not(target_os = "windows"))]
        self.run_polling().await
    }

    #[cfg(target_os = "windows")]
    async fn run_windows(self) -> anyhow::Result<()> {
        let mut signals = start_windows_listener();
        while let Some(signal) = signals.recv().await {
            signal.map_err(|err| anyhow::anyhow!("clipboard listener stopped: {err}"))?;
            if self.has_active_target() {
                self.process_local_change().await?;
            }
        }

        anyhow::bail!("clipboard listener channel closed")
    }

    #[cfg(not(target_os = "windows"))]
    async fn run_polling(self) -> anyhow::Result<()> {
        let mut interval = time::interval(CLIPBOARD_POLL_INTERVAL);
        loop {
            interval.tick().await;
            if !self.has_active_target() {
                continue;
            }
            self.process_local_change().await?;
        }
    }

    async fn process_local_change(&self) -> anyhow::Result<()> {
        let payload = tokio::task::spawn_blocking(read_system_clipboard)
            .await
            .map_err(|err| anyhow::anyhow!("clipboard worker stopped: {err}"))?;
        let Ok(Some(payload)) = payload else {
            return Ok(());
        };

        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let event = match event_from_payload(
            self.local_device.id.clone(),
            sequence,
            timestamp_ms(),
            payload,
        ) {
            Ok(event) => event,
            Err(err) => {
                tracing::debug!(?err, "ignored local clipboard payload");
                return Ok(());
            }
        };
        if !self.tracker.lock().unwrap().observe_local(&event) {
            return Ok(());
        }

        let peer_ids = self
            .settings
            .lock()
            .unwrap()
            .paired_peers
            .iter()
            .filter(|peer| peer.send_clipboard && self.transport.is_connected(&peer.device.id))
            .map(|peer| peer.device.id.clone())
            .collect::<Vec<_>>();
        for peer_id in peer_ids {
            if let Err(err) = self
                .transport
                .send_message(&peer_id, TransportMessage::Clipboard(event.clone()))
                .await
            {
                tracing::debug!(?err, device_id = ?peer_id, "failed to send clipboard event");
            }
        }

        Ok(())
    }

    fn has_active_target(&self) -> bool {
        let settings = self.settings.lock().unwrap().clone();
        has_active_clipboard_target(&settings, |peer_id| self.transport.is_connected(peer_id))
    }

    pub fn handle_remote(
        &self,
        peer_id: &DeviceId,
        event: ClipboardEvent,
    ) -> Result<bool, ClipboardError> {
        if event.source_device_id != *peer_id {
            return Err(ClipboardError::UnauthorizedSource);
        }
        let receive_enabled = self
            .settings
            .lock()
            .unwrap()
            .paired_peers
            .iter()
            .find(|peer| peer.device.id == *peer_id)
            .is_some_and(|peer| peer.receive_clipboard);
        if !receive_enabled || !self.tracker.lock().unwrap().accept_remote(&event) {
            return Ok(false);
        }

        write_system_clipboard(&event.payload)?;
        Ok(true)
    }
}

#[cfg(target_os = "windows")]
fn start_windows_listener() -> mpsc::UnboundedReceiver<Result<(), String>> {
    let (sender, receiver) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut monitor = match clipboard_win::Monitor::new() {
            Ok(monitor) => monitor,
            Err(err) => {
                let _ = sender.send(Err(format!("{err:?}")));
                return;
            }
        };

        for result in &mut monitor {
            match result {
                Ok(true) => {
                    if sender.send(Ok(())).is_err() {
                        break;
                    }
                }
                Ok(false) => break,
                Err(err) => {
                    let _ = sender.send(Err(format!("{err:?}")));
                    break;
                }
            }
        }
    });
    receiver
}

fn has_active_clipboard_target<F>(settings: &LocalSettings, is_connected: F) -> bool
where
    F: Fn(&DeviceId) -> bool,
{
    settings
        .paired_peers
        .iter()
        .any(|peer| peer.send_clipboard && is_connected(&peer.device.id))
}

fn event_from_payload(
    source_device_id: DeviceId,
    sequence: u64,
    timestamp_ms: u64,
    payload: ClipboardPayload,
) -> Result<ClipboardEvent, ClipboardError> {
    match payload {
        ClipboardPayload::Text { text } => Ok(ClipboardEvent::from_text(
            source_device_id,
            sequence,
            timestamp_ms,
            text,
        )),
        ClipboardPayload::Image {
            width,
            height,
            data,
            ..
        } => ClipboardEvent::from_image(
            source_device_id,
            sequence,
            timestamp_ms,
            width,
            height,
            data,
        ),
    }
}

fn validate_image(width: u32, height: u32, data: &[u8]) -> Result<(), ClipboardError> {
    if width == 0 || height == 0 || data.len() > MAX_IMAGE_BYTES {
        return if data.len() > MAX_IMAGE_BYTES {
            Err(ClipboardError::ImageTooLarge(data.len()))
        } else {
            Err(ClipboardError::InvalidImage)
        };
    }
    Ok(())
}

fn content_hash(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn read_system_clipboard() -> Result<Option<ClipboardPayload>, ClipboardError> {
    let mut clipboard = Clipboard::new().map_err(|err| ClipboardError::System(err.to_string()))?;
    if let Ok(text) = clipboard.get_text() {
        return Ok(Some(ClipboardPayload::Text { text }));
    }
    if let Ok(image) = clipboard.get_image() {
        let data = image.bytes.into_owned();
        validate_image(image.width as u32, image.height as u32, &data)?;
        return Ok(Some(ClipboardPayload::Image {
            mime_type: "image/raw-rgba".to_string(),
            width: image.width as u32,
            height: image.height as u32,
            data,
        }));
    }
    Ok(None)
}

fn write_system_clipboard(payload: &ClipboardPayload) -> Result<(), ClipboardError> {
    let mut clipboard = Clipboard::new().map_err(|err| ClipboardError::System(err.to_string()))?;
    match payload {
        ClipboardPayload::Text { text } => clipboard
            .set_text(text.clone())
            .map_err(|err| ClipboardError::System(err.to_string())),
        ClipboardPayload::Image {
            width,
            height,
            data,
            ..
        } => {
            validate_image(*width, *height, data)?;
            clipboard
                .set_image(ImageData {
                    width: *width as usize,
                    height: *height as usize,
                    bytes: Cow::Owned(data.clone()),
                })
                .map_err(|err| ClipboardError::System(err.to_string()))
        }
    }
}

mod base64_bytes {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DeviceInfo, PairedPeer, PeerConnectionState};

    fn settings_with_peer(send_clipboard: bool) -> LocalSettings {
        LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: vec![PairedPeer {
                device: DeviceInfo::new_local("MacBook", 45731),
                receive_clipboard: true,
                send_clipboard,
                is_default_file_target: false,
                state: PeerConnectionState::Offline,
            }],
            ui_locale: "zh-CN".to_string(),
        }
    }

    #[test]
    fn clipboard_polling_is_disabled_without_paired_devices() {
        let settings = LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: Vec::new(),
            ui_locale: "zh-CN".to_string(),
        };

        assert!(!has_active_clipboard_target(&settings, |_| true));
    }

    #[test]
    fn clipboard_polling_requires_connected_receiver() {
        let settings = settings_with_peer(true);

        assert!(!has_active_clipboard_target(&settings, |_| false));
        assert!(has_active_clipboard_target(&settings, |_| true));
    }

    #[test]
    fn clipboard_polling_ignores_peers_that_disabled_sending() {
        let settings = settings_with_peer(false);

        assert!(!has_active_clipboard_target(&settings, |_| true));
    }

    #[test]
    fn text_event_has_id_sequence_timestamp_and_content_hash() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);

        let event = ClipboardEvent::from_text(device.id, 7, 1_700_000_000_000, "hello");

        assert!(!event.event_id.is_empty());
        assert_eq!(event.sequence, 7);
        assert_eq!(event.timestamp_ms, 1_700_000_000_000);
        assert_eq!(event.content_hash, content_hash(b"text:hello"));
        assert!(matches!(event.payload, ClipboardPayload::Text { ref text } if text == "hello"));
    }

    #[test]
    fn oversized_image_is_rejected_without_creating_an_event() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let image = vec![0_u8; MAX_IMAGE_BYTES + 1];

        let result = ClipboardEvent::from_image(device.id, 1, 10, 2, 2, image);

        assert!(
            matches!(result, Err(ClipboardError::ImageTooLarge(size)) if size == MAX_IMAGE_BYTES + 1)
        );
    }

    #[test]
    fn remote_event_is_accepted_once_by_content_hash() {
        let device = DeviceInfo::new_local("MacBook", 45731);
        let event = ClipboardEvent::from_text(device.id, 1, 10, "hello");
        let mut tracker = ClipboardTracker::default();

        assert!(tracker.accept_remote(&event));
        assert!(!tracker.accept_remote(&event));
    }

    #[test]
    fn different_clipboard_contents_are_not_deduplicated() {
        let device = DeviceInfo::new_local("MacBook", 45731);
        let first = ClipboardEvent::from_text(device.id.clone(), 1, 10, "first");
        let second = ClipboardEvent::from_text(device.id, 2, 11, "second");
        let mut tracker = ClipboardTracker::default();

        assert!(tracker.accept_remote(&first));
        assert!(tracker.accept_remote(&second));
    }
}
