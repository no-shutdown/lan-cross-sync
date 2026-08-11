use crate::{
    domain::{DeviceId, DeviceInfo, LocalSettings},
    transport::{TransportMessage, TransportRuntime},
};
use arboard::{Clipboard, ImageData};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{
    codecs::png::{PngDecoder, PngEncoder},
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, Limits,
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    io::Cursor,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
#[cfg(target_os = "windows")]
use tokio::sync::mpsc;
#[cfg(not(target_os = "windows"))]
use tokio::time;
use uuid::Uuid;

#[cfg(not(target_os = "windows"))]
pub const CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_RAW_IMAGE_BYTES: usize = 128 * 1024 * 1024;
pub const CLIPBOARD_IMAGE_CHUNK_BYTES: usize = 64 * 1024;
const IMAGE_MIME_TYPE: &str = "image/png";

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
pub struct ClipboardImageStart {
    pub event_id: String,
    pub source_device_id: DeviceId,
    pub timestamp: i64,
    pub content_hash: String,
    pub width: u32,
    pub height: u32,
    pub total_bytes: u64,
    pub mime_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClipboardImageChunk {
    pub event_id: String,
    pub offset: u64,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClipboardImageComplete {
    pub event_id: String,
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
    #[error("raw image is too large: {0} bytes")]
    RawImageTooLarge(u64),
    #[error("image payload has invalid dimensions")]
    InvalidImage,
    #[error("invalid image transfer: {0}")]
    InvalidImageTransfer(String),
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
        decode_png(width, height, &data)?;
        Ok(Self {
            event_id: Uuid::new_v4().to_string(),
            source_device_id,
            sequence,
            timestamp_ms,
            content_hash: image_content_hash(width, height, &data),
            payload: ClipboardPayload::Image {
                mime_type: IMAGE_MIME_TYPE.to_string(),
                width,
                height,
                data,
            },
        })
    }
}

pub struct IncomingImageTransfer {
    event_id: String,
    source_device_id: DeviceId,
    timestamp_ms: u64,
    content_hash: String,
    width: u32,
    height: u32,
    total_bytes: usize,
    png_bytes: Vec<u8>,
}

impl IncomingImageTransfer {
    pub fn from_start(
        peer_id: DeviceId,
        start: ClipboardImageStart,
    ) -> Result<Self, ClipboardError> {
        if start.source_device_id != peer_id {
            return Err(ClipboardError::UnauthorizedSource);
        }
        if start.event_id.is_empty() || start.content_hash.is_empty() {
            return Err(ClipboardError::InvalidImageTransfer(
                "event metadata is incomplete".to_string(),
            ));
        }
        if start.mime_type != IMAGE_MIME_TYPE {
            return Err(ClipboardError::InvalidImageTransfer(
                "unsupported image mime type".to_string(),
            ));
        }
        let timestamp_ms = u64::try_from(start.timestamp).map_err(|_| {
            ClipboardError::InvalidImageTransfer("timestamp is negative".to_string())
        })?;
        validate_dimensions(start.width, start.height)?;
        if start.total_bytes == 0 {
            return Err(ClipboardError::InvalidImageTransfer(
                "image transfer is empty".to_string(),
            ));
        }
        if start.total_bytes > MAX_IMAGE_BYTES as u64 {
            return Err(ClipboardError::ImageTooLarge(
                usize::try_from(start.total_bytes).unwrap_or(usize::MAX),
            ));
        }
        let total_bytes = usize::try_from(start.total_bytes)
            .map_err(|_| ClipboardError::ImageTooLarge(usize::MAX))?;

        Ok(Self {
            event_id: start.event_id,
            source_device_id: start.source_device_id,
            timestamp_ms,
            content_hash: start.content_hash,
            width: start.width,
            height: start.height,
            total_bytes,
            png_bytes: Vec::with_capacity(total_bytes),
        })
    }

    pub fn accept_chunk(&mut self, offset: u64, data: Vec<u8>) -> Result<(), ClipboardError> {
        if data.is_empty() {
            return Err(ClipboardError::InvalidImageTransfer(
                "image chunk is empty".to_string(),
            ));
        }
        if data.len() > CLIPBOARD_IMAGE_CHUNK_BYTES {
            return Err(ClipboardError::InvalidImageTransfer(
                "image chunk is too large".to_string(),
            ));
        }
        if offset != self.png_bytes.len() as u64 {
            return Err(ClipboardError::InvalidImageTransfer(
                "image chunks must be contiguous".to_string(),
            ));
        }
        let new_len = self
            .png_bytes
            .len()
            .checked_add(data.len())
            .ok_or_else(|| {
                ClipboardError::InvalidImageTransfer("image size overflow".to_string())
            })?;
        if new_len > self.total_bytes || new_len > MAX_IMAGE_BYTES {
            return Err(ClipboardError::InvalidImageTransfer(
                "image transfer exceeds declared size".to_string(),
            ));
        }
        self.png_bytes.extend_from_slice(&data);
        Ok(())
    }

    pub fn finish(self) -> Result<ClipboardEvent, ClipboardError> {
        if self.png_bytes.len() != self.total_bytes {
            return Err(ClipboardError::InvalidImageTransfer(
                "image transfer is incomplete".to_string(),
            ));
        }
        decode_png(self.width, self.height, &self.png_bytes)?;
        if image_content_hash(self.width, self.height, &self.png_bytes) != self.content_hash {
            return Err(ClipboardError::InvalidImageTransfer(
                "image content hash does not match".to_string(),
            ));
        }
        Ok(ClipboardEvent {
            event_id: self.event_id,
            source_device_id: self.source_device_id,
            sequence: 0,
            timestamp_ms: self.timestamp_ms,
            content_hash: self.content_hash,
            payload: ClipboardPayload::Image {
                mime_type: IMAGE_MIME_TYPE.to_string(),
                width: self.width,
                height: self.height,
                data: self.png_bytes,
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
    incoming_images: Arc<Mutex<HashMap<(DeviceId, String), IncomingImageTransfer>>>,
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
            incoming_images: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        #[cfg(target_os = "windows")]
        {
            self.run_windows().await
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
        let payload = tokio::task::spawn_blocking(read_system_clipboard_with_retry)
            .await
            .map_err(|err| anyhow::anyhow!("clipboard worker stopped: {err}"))??;
        let Some(payload) = payload else {
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
            if let Err(err) = self.send_clipboard_event(&peer_id, &event).await {
                tracing::debug!(?err, device_id = ?peer_id, "failed to send clipboard event");
            }
        }

        Ok(())
    }

    fn has_active_target(&self) -> bool {
        let settings = self.settings.lock().unwrap().clone();
        has_active_clipboard_target(&settings, |peer_id| self.transport.is_connected(peer_id))
    }

    async fn send_clipboard_event(
        &self,
        peer_id: &DeviceId,
        event: &ClipboardEvent,
    ) -> anyhow::Result<()> {
        match &event.payload {
            ClipboardPayload::Text { .. } => {
                self.transport
                    .send_message(peer_id, TransportMessage::Clipboard(event.clone()))
                    .await?;
            }
            ClipboardPayload::Image {
                mime_type,
                width,
                height,
                data,
            } => {
                if mime_type != IMAGE_MIME_TYPE {
                    return Err(anyhow::anyhow!(ClipboardError::InvalidImage));
                }
                decode_png(*width, *height, data).map_err(|err| anyhow::anyhow!(err))?;
                let timestamp = i64::try_from(event.timestamp_ms)
                    .map_err(|_| anyhow::anyhow!("clipboard timestamp is out of range"))?;
                let start = ClipboardImageStart {
                    event_id: event.event_id.clone(),
                    source_device_id: event.source_device_id.clone(),
                    timestamp,
                    content_hash: event.content_hash.clone(),
                    width: *width,
                    height: *height,
                    total_bytes: data.len() as u64,
                    mime_type: mime_type.clone(),
                };
                self.transport
                    .send_message(peer_id, TransportMessage::ClipboardImageStart(start))
                    .await?;
                for chunk in
                    plan_image_chunks(&event.event_id, data).map_err(|err| anyhow::anyhow!(err))?
                {
                    self.transport
                        .send_message(peer_id, TransportMessage::ClipboardImageChunk(chunk))
                        .await?;
                }
                self.transport
                    .send_message(
                        peer_id,
                        TransportMessage::ClipboardImageComplete(ClipboardImageComplete {
                            event_id: event.event_id.clone(),
                        }),
                    )
                    .await?;
            }
        }
        Ok(())
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

    pub fn handle_image_start(
        &self,
        peer_id: &DeviceId,
        start: ClipboardImageStart,
    ) -> Result<bool, ClipboardError> {
        if !self.receive_clipboard_enabled(peer_id) {
            return Ok(false);
        }
        let event_id = start.event_id.clone();
        let transfer = IncomingImageTransfer::from_start(peer_id.clone(), start)?;
        self.incoming_images
            .lock()
            .unwrap()
            .insert((peer_id.clone(), event_id), transfer);
        Ok(true)
    }

    pub fn handle_image_chunk(
        &self,
        peer_id: &DeviceId,
        chunk: ClipboardImageChunk,
    ) -> Result<bool, ClipboardError> {
        let key = (peer_id.clone(), chunk.event_id.clone());
        let mut transfers = self.incoming_images.lock().unwrap();
        let Some(transfer) = transfers.get_mut(&key) else {
            return Ok(false);
        };
        if let Err(err) = transfer.accept_chunk(chunk.offset, chunk.data) {
            transfers.remove(&key);
            return Err(err);
        }
        Ok(true)
    }

    pub fn handle_image_complete(
        &self,
        peer_id: &DeviceId,
        complete: ClipboardImageComplete,
    ) -> Result<bool, ClipboardError> {
        let key = (peer_id.clone(), complete.event_id);
        let Some(transfer) = self.incoming_images.lock().unwrap().remove(&key) else {
            return Ok(false);
        };
        let event = transfer.finish()?;
        self.handle_remote(peer_id, event)
    }

    pub fn handle_peer_disconnected(&self, peer_id: &DeviceId) {
        self.incoming_images
            .lock()
            .unwrap()
            .retain(|(transfer_peer_id, _), _| transfer_peer_id != peer_id);
    }

    fn receive_clipboard_enabled(&self, peer_id: &DeviceId) -> bool {
        self.settings
            .lock()
            .unwrap()
            .paired_peers
            .iter()
            .find(|peer| peer.device.id == *peer_id)
            .is_some_and(|peer| peer.receive_clipboard)
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
            mime_type,
            width,
            height,
            data,
        } => {
            if mime_type != IMAGE_MIME_TYPE {
                return Err(ClipboardError::InvalidImage);
            }
            ClipboardEvent::from_image(
                source_device_id,
                sequence,
                timestamp_ms,
                width,
                height,
                data,
            )
        }
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<usize, ClipboardError> {
    if width == 0 || height == 0 {
        return Err(ClipboardError::InvalidImage);
    }
    let raw_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ClipboardError::InvalidImage)?;
    if raw_bytes > MAX_RAW_IMAGE_BYTES as u64 {
        return Err(ClipboardError::RawImageTooLarge(raw_bytes));
    }
    usize::try_from(raw_bytes).map_err(|_| ClipboardError::RawImageTooLarge(raw_bytes))
}

fn validate_raw_rgba(width: u32, height: u32, raw_rgba: &[u8]) -> Result<(), ClipboardError> {
    if raw_rgba.len() > MAX_RAW_IMAGE_BYTES {
        return Err(ClipboardError::RawImageTooLarge(raw_rgba.len() as u64));
    }
    let expected_bytes = validate_dimensions(width, height)?;
    if raw_rgba.len() != expected_bytes {
        return Err(ClipboardError::InvalidImage);
    }
    Ok(())
}

pub fn encode_png(width: u32, height: u32, raw_rgba: &[u8]) -> Result<Vec<u8>, ClipboardError> {
    validate_raw_rgba(width, height, raw_rgba)?;
    let mut png_bytes = Vec::new();
    PngEncoder::new(&mut png_bytes)
        .write_image(raw_rgba, width, height, ExtendedColorType::Rgba8)
        .map_err(|_| ClipboardError::InvalidImage)?;
    if png_bytes.len() > MAX_IMAGE_BYTES {
        return Err(ClipboardError::ImageTooLarge(png_bytes.len()));
    }
    Ok(png_bytes)
}

pub fn decode_png(width: u32, height: u32, png_bytes: &[u8]) -> Result<Vec<u8>, ClipboardError> {
    if png_bytes.len() > MAX_IMAGE_BYTES {
        return Err(ClipboardError::ImageTooLarge(png_bytes.len()));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some(MAX_RAW_IMAGE_BYTES as u64);
    let expected_raw_bytes = validate_dimensions(width, height)?;
    let decoder = PngDecoder::with_limits(Cursor::new(png_bytes), limits)
        .map_err(|_| ClipboardError::InvalidImage)?;
    if decoder.dimensions() != (width, height) {
        return Err(ClipboardError::InvalidImage);
    }
    let image = DynamicImage::from_decoder(decoder).map_err(|_| ClipboardError::InvalidImage)?;
    let raw_rgba = image.into_rgba8().into_raw();
    if raw_rgba.len() > MAX_RAW_IMAGE_BYTES {
        return Err(ClipboardError::RawImageTooLarge(raw_rgba.len() as u64));
    }
    if raw_rgba.len() != expected_raw_bytes {
        return Err(ClipboardError::InvalidImage);
    }
    Ok(raw_rgba)
}

fn image_content_hash(width: u32, height: u32, png_bytes: &[u8]) -> String {
    let mut hash_input = Vec::with_capacity(24 + png_bytes.len());
    hash_input.extend_from_slice(format!("image:{width}x{height}:").as_bytes());
    hash_input.extend_from_slice(png_bytes);
    content_hash(&hash_input)
}

fn plan_image_chunks(
    event_id: &str,
    png_bytes: &[u8],
) -> Result<Vec<ClipboardImageChunk>, ClipboardError> {
    if event_id.is_empty() || png_bytes.is_empty() {
        return Err(ClipboardError::InvalidImage);
    }
    if png_bytes.len() > MAX_IMAGE_BYTES {
        return Err(ClipboardError::ImageTooLarge(png_bytes.len()));
    }
    Ok(png_bytes
        .chunks(CLIPBOARD_IMAGE_CHUNK_BYTES)
        .enumerate()
        .map(|(index, data)| ClipboardImageChunk {
            event_id: event_id.to_string(),
            offset: (index * CLIPBOARD_IMAGE_CHUNK_BYTES) as u64,
            data: data.to_vec(),
        })
        .collect())
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

fn retry_clipboard_read<T, E, F, W>(mut read: F, mut wait: W) -> Result<Option<T>, E>
where
    F: FnMut() -> Result<Option<T>, E>,
    W: FnMut(Duration),
{
    let mut result = read();
    for _ in 1..5 {
        if matches!(&result, Ok(Some(_))) {
            return result;
        }
        wait(Duration::from_millis(20));
        result = read();
    }
    result
}

fn read_system_clipboard() -> Result<Option<ClipboardPayload>, ClipboardError> {
    let mut clipboard = Clipboard::new().map_err(|err| ClipboardError::System(err.to_string()))?;
    if let Ok(text) = clipboard.get_text() {
        return Ok(Some(ClipboardPayload::Text { text }));
    }
    if let Ok(image) = clipboard.get_image() {
        let width = u32::try_from(image.width).map_err(|_| ClipboardError::InvalidImage)?;
        let height = u32::try_from(image.height).map_err(|_| ClipboardError::InvalidImage)?;
        let raw_rgba = image.bytes.into_owned();
        let data = encode_png(width, height, &raw_rgba)?;
        return Ok(Some(ClipboardPayload::Image {
            mime_type: IMAGE_MIME_TYPE.to_string(),
            width,
            height,
            data,
        }));
    }
    Ok(None)
}

fn read_system_clipboard_with_retry() -> Result<Option<ClipboardPayload>, ClipboardError> {
    retry_clipboard_read(read_system_clipboard, std::thread::sleep)
}

fn write_system_clipboard(payload: &ClipboardPayload) -> Result<(), ClipboardError> {
    match payload {
        ClipboardPayload::Text { text } => {
            let mut clipboard =
                Clipboard::new().map_err(|err| ClipboardError::System(err.to_string()))?;
            clipboard
                .set_text(text.clone())
                .map_err(|err| ClipboardError::System(err.to_string()))
        }
        ClipboardPayload::Image {
            mime_type,
            width,
            height,
            data,
        } => {
            if mime_type != IMAGE_MIME_TYPE {
                return Err(ClipboardError::InvalidImage);
            }
            let raw_rgba = decode_png(*width, *height, data)?;
            let mut clipboard =
                Clipboard::new().map_err(|err| ClipboardError::System(err.to_string()))?;
            clipboard
                .set_image(ImageData {
                    width: *width as usize,
                    height: *height as usize,
                    bytes: Cow::Owned(raw_rgba),
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
    use crate::registry::PeerRegistry;

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
            discoverable_enabled: true,
            search_enabled: true,
        }
    }

    fn settings_with_receiving_peers(peers: &[DeviceInfo]) -> LocalSettings {
        LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: peers
                .iter()
                .cloned()
                .map(|device| PairedPeer {
                    device,
                    receive_clipboard: true,
                    send_clipboard: true,
                    is_default_file_target: false,
                    state: PeerConnectionState::Connected,
                })
                .collect(),
            ui_locale: "zh-CN".to_string(),
            discoverable_enabled: true,
            search_enabled: true,
        }
    }

    fn receiving_service(peers: &[DeviceInfo]) -> ClipboardService {
        let settings = settings_with_receiving_peers(peers);
        let registry = Arc::new(Mutex::new(PeerRegistry::from_paired(
            settings.paired_peers.clone(),
        )));
        let (transport, _) = TransportRuntime::new(settings.local_device.clone(), registry);
        ClipboardService::new(
            settings.local_device.clone(),
            Arc::new(Mutex::new(settings)),
            Arc::new(transport),
        )
    }

    fn test_png() -> (u32, u32, Vec<u8>) {
        let width = 2;
        let height = 2;
        let raw = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ];
        (width, height, encode_png(width, height, &raw).unwrap())
    }

    fn image_start(
        peer_id: DeviceId,
        event_id: &str,
        width: u32,
        height: u32,
        png: &[u8],
    ) -> ClipboardImageStart {
        ClipboardImageStart {
            event_id: event_id.to_string(),
            source_device_id: peer_id,
            timestamp: 1_700_000_000_000,
            content_hash: image_content_hash(width, height, png),
            width,
            height,
            total_bytes: png.len() as u64,
            mime_type: IMAGE_MIME_TYPE.to_string(),
        }
    }

    #[test]
    fn png_encode_decode_round_trips_rgba_pixels_and_dimensions() {
        let (width, height, png) = test_png();

        assert_eq!(
            decode_png(width, height, &png).unwrap(),
            vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,]
        );
    }

    #[test]
    fn png_limits_and_malformed_or_mismatched_dimensions_are_rejected() {
        let (_, _, png) = test_png();

        assert!(matches!(
            encode_png(0, 1, &[0, 0, 0, 0]),
            Err(ClipboardError::InvalidImage)
        ));
        assert!(matches!(
            encode_png(1, 1, &[]),
            Err(ClipboardError::InvalidImage)
        ));
        assert!(matches!(
            encode_png(131_073, 256, &[0]),
            Err(ClipboardError::RawImageTooLarge(_))
        ));
        assert!(matches!(
            decode_png(2, 2, b"not a png"),
            Err(ClipboardError::InvalidImage)
        ));
        assert!(matches!(
            decode_png(1, 2, &png),
            Err(ClipboardError::InvalidImage)
        ));
        assert!(matches!(
            decode_png(131_073, 256, &[]),
            Err(ClipboardError::RawImageTooLarge(_))
        ));
    }

    #[test]
    fn image_event_uses_png_mime_and_hashes_dimensions_and_png_bytes() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let (width, height, png) = test_png();

        let event =
            ClipboardEvent::from_image(device.id, 4, 1_700_000_000_000, width, height, png.clone())
                .unwrap();

        assert!(matches!(
            event.payload,
            ClipboardPayload::Image {
                ref mime_type,
                width: event_width,
                height: event_height,
                ref data,
            } if mime_type == IMAGE_MIME_TYPE
                && event_width == width
                && event_height == height
                && data == &png
        ));
        assert_eq!(event.content_hash, image_content_hash(width, height, &png));
    }

    #[test]
    fn image_chunk_plan_has_contiguous_offsets_and_bounded_chunks() {
        let png = vec![7_u8; CLIPBOARD_IMAGE_CHUNK_BYTES * 2 + 3];

        let chunks = plan_image_chunks("event-1", &png).unwrap();

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].offset, 0);
        assert_eq!(chunks[1].offset, CLIPBOARD_IMAGE_CHUNK_BYTES as u64);
        assert_eq!(chunks[2].offset, (CLIPBOARD_IMAGE_CHUNK_BYTES * 2) as u64);
        assert!(
            chunks
                .iter()
                .all(|chunk| !chunk.data.is_empty()
                    && chunk.data.len() <= CLIPBOARD_IMAGE_CHUNK_BYTES)
        );
        let rebuilt = chunks
            .iter()
            .flat_map(|chunk| chunk.data.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(rebuilt, png);
    }

    #[test]
    fn incoming_image_transfer_assembles_boundary_chunks_and_preserves_metadata() {
        let peer = DeviceInfo::new_local("MacBook", 45731);
        let (width, height, png) = test_png();
        let start = image_start(peer.id.clone(), "image-event", width, height, &png);
        let mut transfer = IncomingImageTransfer::from_start(peer.id.clone(), start).unwrap();

        for chunk in plan_image_chunks("image-event", &png).unwrap() {
            transfer.accept_chunk(chunk.offset, chunk.data).unwrap();
        }

        let event = transfer.finish().unwrap();
        assert_eq!(event.event_id, "image-event");
        assert_eq!(event.source_device_id, peer.id);
        assert_eq!(event.timestamp_ms, 1_700_000_000_000);
        assert_eq!(event.sequence, 0);
        assert_eq!(event.content_hash, image_content_hash(width, height, &png));
        assert!(matches!(
            event.payload,
            ClipboardPayload::Image { data, .. } if data == png
        ));
    }

    #[test]
    fn incoming_image_transfer_rejects_gap_overlap_oversized_and_out_of_bounds_chunks() {
        let peer = DeviceInfo::new_local("MacBook", 45731);
        let (width, height, png) = test_png();

        let mut gap = IncomingImageTransfer::from_start(
            peer.id.clone(),
            image_start(peer.id.clone(), "gap", width, height, &png),
        )
        .unwrap();
        assert!(gap.accept_chunk(1, vec![1]).is_err());

        let mut overlap = IncomingImageTransfer::from_start(
            peer.id.clone(),
            image_start(peer.id.clone(), "overlap", width, height, &png),
        )
        .unwrap();
        overlap.accept_chunk(0, vec![1, 2]).unwrap();
        assert!(overlap.accept_chunk(0, vec![3]).is_err());

        let mut oversized = IncomingImageTransfer::from_start(
            peer.id.clone(),
            image_start(peer.id.clone(), "oversized", width, height, &png),
        )
        .unwrap();
        assert!(oversized
            .accept_chunk(0, vec![0; CLIPBOARD_IMAGE_CHUNK_BYTES + 1])
            .is_err());

        let mut out_of_bounds = IncomingImageTransfer::from_start(
            peer.id.clone(),
            ClipboardImageStart {
                total_bytes: 2,
                ..image_start(peer.id.clone(), "bounds", width, height, &png)
            },
        )
        .unwrap();
        assert!(out_of_bounds.accept_chunk(0, vec![1, 2, 3]).is_err());
    }

    #[test]
    fn incoming_image_transfer_requires_complete_data_and_matching_content_hash() {
        let peer = DeviceInfo::new_local("MacBook", 45731);
        let (width, height, png) = test_png();

        let mut incomplete = IncomingImageTransfer::from_start(
            peer.id.clone(),
            image_start(peer.id.clone(), "incomplete", width, height, &png),
        )
        .unwrap();
        incomplete.accept_chunk(0, vec![1]).unwrap();
        assert!(incomplete.finish().is_err());

        let mut mismatched_start = image_start(peer.id.clone(), "mismatch", width, height, &png);
        mismatched_start.content_hash = "wrong-hash".to_string();
        let mut mismatched = IncomingImageTransfer::from_start(peer.id, mismatched_start).unwrap();
        for chunk in plan_image_chunks("mismatch", &png).unwrap() {
            mismatched.accept_chunk(chunk.offset, chunk.data).unwrap();
        }
        assert!(mismatched.finish().is_err());
    }

    #[test]
    fn incoming_image_transfers_are_isolated_by_peer_and_event_id() {
        let first_peer = DeviceInfo::new_local("MacBook", 45731);
        let second_peer = DeviceInfo::new_local("Linux Desk", 45732);
        let service = receiving_service(&[first_peer.clone(), second_peer.clone()]);
        let (width, height, first_png) = test_png();
        let second_png = encode_png(
            width,
            height,
            &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
        )
        .unwrap();

        service
            .handle_image_start(
                &first_peer.id,
                image_start(
                    first_peer.id.clone(),
                    "same-event",
                    width,
                    height,
                    &first_png,
                ),
            )
            .unwrap();
        service
            .handle_image_start(
                &second_peer.id,
                image_start(
                    second_peer.id.clone(),
                    "same-event",
                    width,
                    height,
                    &second_png,
                ),
            )
            .unwrap();

        service
            .handle_image_chunk(
                &first_peer.id,
                ClipboardImageChunk {
                    event_id: "same-event".to_string(),
                    offset: 0,
                    data: vec![1],
                },
            )
            .unwrap();
        service
            .handle_image_chunk(
                &second_peer.id,
                ClipboardImageChunk {
                    event_id: "same-event".to_string(),
                    offset: 0,
                    data: vec![2],
                },
            )
            .unwrap();

        let transfers = service.incoming_images.lock().unwrap();
        assert_eq!(transfers.len(), 2);
        assert_eq!(
            transfers
                .get(&(first_peer.id.clone(), "same-event".to_string()))
                .unwrap()
                .png_bytes,
            vec![1]
        );
        assert_eq!(
            transfers
                .get(&(second_peer.id.clone(), "same-event".to_string()))
                .unwrap()
                .png_bytes,
            vec![2]
        );

        drop(transfers);
        service.handle_peer_disconnected(&first_peer.id);
        assert_eq!(service.incoming_images.lock().unwrap().len(), 1);
        assert!(service
            .incoming_images
            .lock()
            .unwrap()
            .contains_key(&(second_peer.id, "same-event".to_string())));
    }

    #[test]
    fn clipboard_polling_is_disabled_without_paired_devices() {
        let settings = LocalSettings {
            local_device: DeviceInfo::new_local("Windows Desk", 45731),
            paired_peers: Vec::new(),
            ui_locale: "zh-CN".to_string(),
            discoverable_enabled: true,
            search_enabled: true,
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

    #[test]
    fn clipboard_read_retries_error_until_payload_is_available() {
        let mut outcomes = vec![
            Err(ClipboardError::System("busy".to_string())),
            Ok(Some(ClipboardPayload::Text {
                text: "ready".to_string(),
            })),
        ]
        .into_iter();
        let mut attempts = 0;
        let mut waits = Vec::new();

        let result = retry_clipboard_read(
            || {
                attempts += 1;
                outcomes.next().expect("test outcome missing")
            },
            |delay| waits.push(delay),
        );

        assert!(matches!(
            result,
            Ok(Some(ClipboardPayload::Text { ref text })) if text == "ready"
        ));
        assert_eq!(attempts, 2);
        assert_eq!(waits, vec![std::time::Duration::from_millis(20)]);
    }

    #[test]
    fn clipboard_read_retries_empty_results_five_times() {
        let mut attempts = 0;
        let mut waits = Vec::new();

        let result = retry_clipboard_read(
            || {
                attempts += 1;
                Ok::<Option<ClipboardPayload>, ClipboardError>(None)
            },
            |delay| waits.push(delay),
        );

        assert!(matches!(result, Ok(None)));
        assert_eq!(attempts, 5);
        assert_eq!(waits, vec![std::time::Duration::from_millis(20); 4]);
    }

    #[test]
    fn clipboard_read_returns_the_last_error_after_five_attempts() {
        let mut attempts = 0;
        let mut waits = Vec::new();

        let result = retry_clipboard_read(
            || {
                attempts += 1;
                Err::<Option<ClipboardPayload>, ClipboardError>(ClipboardError::System(format!(
                    "attempt {attempts}"
                )))
            },
            |delay| waits.push(delay),
        );

        assert!(matches!(
            result,
            Err(ClipboardError::System(message)) if message == "attempt 5"
        ));
        assert_eq!(attempts, 5);
        assert_eq!(waits, vec![std::time::Duration::from_millis(20); 4]);
    }

    #[test]
    fn clipboard_read_returns_first_payload_without_waiting() {
        let mut attempts = 0;
        let mut waits = Vec::new();

        let result = retry_clipboard_read(
            || {
                attempts += 1;
                Ok::<Option<ClipboardPayload>, ClipboardError>(Some(ClipboardPayload::Text {
                    text: "ready".to_string(),
                }))
            },
            |delay| waits.push(delay),
        );

        assert!(matches!(
            result,
            Ok(Some(ClipboardPayload::Text { ref text })) if text == "ready"
        ));
        assert_eq!(attempts, 1);
        assert!(waits.is_empty());
    }
}
