use crate::{
    clipboard::ClipboardEvent,
    domain::{DeviceId, DeviceInfo, PeerConnectionState},
    file_transfer::{FileAccept, FileCancel, FileChunk, FileComplete, FileOffer},
    protocol::PROTOCOL_VERSION,
    registry::PeerRegistry,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io,
    sync::{Arc, Mutex},
    time::Instant,
};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::{self, Duration},
};
use uuid::Uuid;

pub const MAX_CONTROL_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn bind_transport_listener(preferred_port: u16) -> Result<(TcpListener, u16, bool)> {
    match TcpListener::bind(("0.0.0.0", preferred_port)).await {
        Ok(listener) => {
            let port = listener
                .local_addr()
                .context("failed to read TCP transport listener address")?
                .port();
            Ok((listener, port, false))
        }
        Err(preferred_error) => {
            tracing::warn!(
                preferred_port,
                ?preferred_error,
                "preferred TCP transport port is unavailable; selecting an OS-assigned port"
            );
            let listener = TcpListener::bind(("0.0.0.0", 0))
                .await
                .context("failed to bind an OS-assigned TCP transport port")?;
            let port = listener
                .local_addr()
                .context("failed to read fallback TCP transport listener address")?
                .port();
            Ok((listener, port, true))
        }
    }
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("transport I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("control frame is empty")]
    EmptyFrame,
    #[error("control frame is too large: {0} bytes")]
    FrameTooLarge(usize),
    #[error("invalid control message: {0}")]
    InvalidMessage(#[from] serde_json::Error),
    #[error("invalid handshake: {0}")]
    InvalidHandshake(String),
    #[error("unsupported transport protocol version: {0}")]
    UnsupportedProtocol(u16),
    #[error("peer is not paired")]
    UnpairedPeer,
    #[error("peer identity does not match the expected device")]
    PeerIdentityMismatch,
    #[error("handshake rejected: {0}")]
    HandshakeRejected(String),
    #[error("heartbeat timed out")]
    HeartbeatTimeout,
    #[error("peer is not connected")]
    NotConnected,
    #[error("connection is closed")]
    ConnectionClosed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportMessage {
    Hello {
        protocol_version: u16,
        connection_id: String,
        from_device: DeviceInfo,
    },
    HelloAck {
        protocol_version: u16,
        connection_id: String,
        accepted: bool,
        from_device: DeviceInfo,
        reason_code: Option<String>,
    },
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    Clipboard(ClipboardEvent),
    FileOffer(FileOffer),
    FileAccept(FileAccept),
    FileChunk(FileChunk),
    FileComplete(FileComplete),
    FileCancel(FileCancel),
    Unpair,
}

pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, TransportError> {
    if payload.is_empty() {
        return Err(TransportError::EmptyFrame);
    }
    if payload.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(TransportError::FrameTooLarge(payload.len()));
    }

    let length =
        u32::try_from(payload.len()).map_err(|_| TransportError::FrameTooLarge(payload.len()))?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

pub async fn write_frame<W>(writer: &mut W, payload: &[u8]) -> Result<(), TransportError>
where
    W: AsyncWrite + Unpin,
{
    let frame = encode_frame(payload)?;
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_frame<R>(reader: &mut R) -> Result<Vec<u8>, TransportError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0_u8; 4];
    reader.read_exact(&mut header).await?;
    let length = u32::from_be_bytes(header) as usize;

    if length == 0 {
        return Err(TransportError::EmptyFrame);
    }
    if length > MAX_CONTROL_FRAME_BYTES {
        return Err(TransportError::FrameTooLarge(length));
    }

    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    Ok(payload)
}

pub async fn write_message<W>(
    writer: &mut W,
    message: &TransportMessage,
) -> Result<(), TransportError>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(message).map_err(TransportError::InvalidMessage)?;
    write_frame(writer, &payload).await
}

pub async fn read_message<R>(reader: &mut R) -> Result<TransportMessage, TransportError>
where
    R: AsyncRead + Unpin,
{
    let payload = read_frame(reader).await?;
    serde_json::from_slice(&payload).map_err(TransportError::InvalidMessage)
}

pub async fn client_handshake<S>(
    stream: &mut S,
    local_device: DeviceInfo,
    expected_peer_id: DeviceId,
) -> Result<DeviceInfo, TransportError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let connection_id = Uuid::new_v4().to_string();
    write_message(
        stream,
        &TransportMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            connection_id: connection_id.clone(),
            from_device: local_device,
        },
    )
    .await?;

    let response = read_message(stream).await?;
    let TransportMessage::HelloAck {
        protocol_version,
        connection_id: response_connection_id,
        accepted,
        from_device,
        reason_code,
    } = response
    else {
        return Err(TransportError::InvalidHandshake(
            "expected hello_ack".to_string(),
        ));
    };

    if protocol_version != PROTOCOL_VERSION {
        return Err(TransportError::UnsupportedProtocol(protocol_version));
    }
    if response_connection_id != connection_id {
        return Err(TransportError::InvalidHandshake(
            "connection id does not match".to_string(),
        ));
    }
    if !accepted {
        return Err(TransportError::HandshakeRejected(
            reason_code.unwrap_or_else(|| "handshake_rejected".to_string()),
        ));
    }
    if from_device.protocol_version != PROTOCOL_VERSION {
        return Err(TransportError::UnsupportedProtocol(
            from_device.protocol_version,
        ));
    }
    if from_device.id != expected_peer_id {
        return Err(TransportError::PeerIdentityMismatch);
    }

    Ok(from_device)
}

pub async fn server_handshake<S, F>(
    stream: &mut S,
    local_device: DeviceInfo,
    is_authorized: F,
) -> Result<DeviceInfo, TransportError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: Fn(&DeviceInfo) -> bool,
{
    let hello = read_message(stream).await?;
    let TransportMessage::Hello {
        protocol_version,
        connection_id,
        from_device,
    } = hello
    else {
        return Err(TransportError::InvalidHandshake(
            "expected hello".to_string(),
        ));
    };

    if protocol_version != PROTOCOL_VERSION || from_device.protocol_version != PROTOCOL_VERSION {
        send_hello_ack(
            stream,
            connection_id,
            local_device,
            false,
            Some("unsupported_protocol".to_string()),
        )
        .await?;
        return Err(TransportError::UnsupportedProtocol(protocol_version));
    }
    if from_device.id == local_device.id {
        send_hello_ack(
            stream,
            connection_id,
            local_device,
            false,
            Some("self_device".to_string()),
        )
        .await?;
        return Err(TransportError::InvalidHandshake(
            "peer is the local device".to_string(),
        ));
    }
    if !is_authorized(&from_device) {
        send_hello_ack(
            stream,
            connection_id,
            local_device,
            false,
            Some("unpaired_peer".to_string()),
        )
        .await?;
        return Err(TransportError::UnpairedPeer);
    }

    send_hello_ack(stream, connection_id, local_device, true, None).await?;
    Ok(from_device)
}

async fn send_hello_ack<S>(
    stream: &mut S,
    connection_id: String,
    local_device: DeviceInfo,
    accepted: bool,
    reason_code: Option<String>,
) -> Result<(), TransportError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_message(
        stream,
        &TransportMessage::HelloAck {
            protocol_version: PROTOCOL_VERSION,
            connection_id,
            accepted,
            from_device: local_device,
            reason_code,
        },
    )
    .await
}

#[derive(Clone, Debug)]
pub enum TransportEvent {
    PeerConnected(DeviceInfo),
    PeerDisconnected {
        peer: DeviceInfo,
        reason_code: String,
    },
    Message {
        peer: DeviceInfo,
        message: TransportMessage,
    },
}

struct ConnectionEntry {
    token: String,
    sender: mpsc::Sender<TransportMessage>,
}

#[derive(Clone)]
pub struct TransportRuntime {
    pub local_device: DeviceInfo,
    pub registry: Arc<Mutex<PeerRegistry>>,
    connections: Arc<Mutex<HashMap<DeviceId, ConnectionEntry>>>,
    connecting: Arc<Mutex<HashSet<DeviceId>>>,
    events: mpsc::UnboundedSender<TransportEvent>,
}

impl TransportRuntime {
    pub fn new(
        local_device: DeviceInfo,
        registry: Arc<Mutex<PeerRegistry>>,
    ) -> (Self, mpsc::UnboundedReceiver<TransportEvent>) {
        let (events, receiver) = mpsc::unbounded_channel();
        (
            Self {
                local_device,
                registry,
                connections: Arc::new(Mutex::new(HashMap::new())),
                connecting: Arc::new(Mutex::new(HashSet::new())),
                events,
            },
            receiver,
        )
    }

    pub async fn listen_loop(self, listener: TcpListener) -> anyhow::Result<()> {
        loop {
            let (stream, _) = listener.accept().await?;
            let runtime = self.clone();
            tokio::spawn(async move {
                if let Err(err) = runtime.accept_connection(stream).await {
                    tracing::debug!(?err, "incoming TCP connection ended");
                }
            });
        }
    }

    pub async fn connect_peer(&self, peer_id: DeviceId) -> anyhow::Result<()> {
        let (endpoint, local_device) = {
            let registry = self.registry.lock().unwrap();
            let endpoint = registry
                .transport_endpoint(&peer_id)
                .ok_or(TransportError::NotConnected)?;
            (endpoint, self.local_device.clone())
        };
        let mut stream = TcpStream::connect(endpoint).await?;
        let peer = client_handshake(&mut stream, local_device, peer_id).await?;
        self.run_connection(stream, peer).await?;
        Ok(())
    }

    pub async fn connect_peer_with_retry(
        &self,
        peer_id: DeviceId,
        attempts: usize,
    ) -> anyhow::Result<()> {
        let attempts = attempts.max(1);
        let mut last_error = None;
        for attempt in 0..attempts {
            match self.connect_peer(peer_id.clone()).await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_error = Some(err);
                    if attempt + 1 < attempts {
                        let delay = Duration::from_millis(250 * (attempt as u64 + 1));
                        time::sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.expect("at least one connection attempt"))
    }

    pub async fn maintain_connections(self) -> anyhow::Result<()> {
        loop {
            let peer_ids = {
                let registry = self.registry.lock().unwrap();
                registry
                    .paired()
                    .into_iter()
                    .filter_map(|peer| {
                        let id = peer.device.id;
                        (registry.transport_endpoint(&id).is_some()
                            && !self.is_connected(&id)
                            && self.mark_connecting(id.clone()))
                        .then_some(id)
                    })
                    .collect::<Vec<_>>()
            };

            for peer_id in peer_ids {
                let runtime = self.clone();
                tokio::spawn(async move {
                    let _ = runtime.connect_peer_with_retry(peer_id.clone(), 3).await;
                    runtime.unmark_connecting(&peer_id);
                });
            }

            time::sleep(Duration::from_secs(5)).await;
        }
    }

    pub async fn send_message(
        &self,
        peer_id: &DeviceId,
        message: TransportMessage,
    ) -> Result<(), TransportError> {
        let sender = self
            .connections
            .lock()
            .unwrap()
            .get(peer_id)
            .map(|entry| entry.sender.clone())
            .ok_or(TransportError::NotConnected)?;
        sender
            .send(message)
            .await
            .map_err(|_| TransportError::ConnectionClosed)
    }

    pub fn is_connected(&self, peer_id: &DeviceId) -> bool {
        self.connections.lock().unwrap().contains_key(peer_id)
    }

    pub fn disconnect_peer(&self, peer_id: &DeviceId) {
        let removed = self.connections.lock().unwrap().remove(peer_id);
        if removed.is_none() {
            return;
        }

        let peer = self.registry.lock().unwrap().device(peer_id);
        self.registry
            .lock()
            .unwrap()
            .set_state(peer_id, PeerConnectionState::Offline);
        if let Some(peer) = peer {
            let _ = self.events.send(TransportEvent::PeerDisconnected {
                peer,
                reason_code: "unpaired_peer".to_string(),
            });
        }
    }

    fn mark_connecting(&self, peer_id: DeviceId) -> bool {
        self.connecting.lock().unwrap().insert(peer_id)
    }

    fn unmark_connecting(&self, peer_id: &DeviceId) {
        self.connecting.lock().unwrap().remove(peer_id);
    }

    async fn accept_connection(&self, mut stream: TcpStream) -> anyhow::Result<()> {
        let local_device = self.local_device.clone();
        let registry = self.registry.clone();
        let peer = server_handshake(&mut stream, local_device, |device| {
            registry
                .lock()
                .map(|registry| registry.is_paired(&device.id))
                .unwrap_or(false)
        })
        .await?;
        self.run_connection(stream, peer).await?;
        Ok(())
    }

    async fn run_connection(&self, stream: TcpStream, peer: DeviceInfo) -> anyhow::Result<()> {
        let token = Uuid::new_v4().to_string();
        let (sender, mut outbound) = mpsc::channel(64);
        self.connections.lock().unwrap().insert(
            peer.id.clone(),
            ConnectionEntry {
                token: token.clone(),
                sender,
            },
        );
        self.registry
            .lock()
            .unwrap()
            .set_state(&peer.id, PeerConnectionState::Connected);
        let _ = self
            .events
            .send(TransportEvent::PeerConnected(peer.clone()));

        let (mut reader, mut writer) = stream.into_split();
        let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);
        let mut last_pong = Instant::now();
        let mut next_nonce = 1_u64;
        let result: Result<(), TransportError> = loop {
            tokio::select! {
                message = read_message(&mut reader) => {
                    match message? {
                        TransportMessage::Ping { nonce } => {
                            write_message(&mut writer, &TransportMessage::Pong { nonce }).await?;
                        }
                        TransportMessage::Pong { .. } => {
                            last_pong = Instant::now();
                        }
                        message => {
                            let _ = self.events.send(TransportEvent::Message {
                                peer: peer.clone(),
                                message,
                            });
                        }
                    }
                }
                message = outbound.recv() => {
                    let Some(message) = message else {
                        break Ok(());
                    };
                    write_message(&mut writer, &message).await?;
                }
                _ = heartbeat.tick() => {
                    if last_pong.elapsed() > HEARTBEAT_TIMEOUT {
                        break Err(TransportError::HeartbeatTimeout);
                    }
                    write_message(&mut writer, &TransportMessage::Ping { nonce: next_nonce }).await?;
                    next_nonce = next_nonce.wrapping_add(1);
                }
            }
        };

        let mut connections = self.connections.lock().unwrap();
        if connections
            .get(&peer.id)
            .is_some_and(|entry| entry.token == token)
        {
            connections.remove(&peer.id);
        }
        drop(connections);
        self.registry
            .lock()
            .unwrap()
            .set_state(&peer.id, PeerConnectionState::Offline);
        let reason_code = result
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_else(|| "connection_closed".to_string());
        let _ = self
            .events
            .send(TransportEvent::PeerDisconnected { peer, reason_code });

        result.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DeviceInfo, PairedPeer};
    use tokio::io::duplex;

    #[test]
    fn frame_prefix_uses_network_byte_order() {
        let frame = encode_frame(b"abc").unwrap();

        assert_eq!(&frame[..4], &[0, 0, 0, 3]);
        assert_eq!(&frame[4..], b"abc");
    }

    #[test]
    fn oversized_control_frame_is_rejected_before_encoding() {
        let payload = vec![0_u8; MAX_CONTROL_FRAME_BYTES + 1];

        assert!(matches!(
            encode_frame(&payload),
            Err(TransportError::FrameTooLarge(size)) if size == payload.len()
        ));
    }

    #[tokio::test]
    async fn framed_io_handles_a_partial_stream() {
        let (mut writer, mut reader) = duplex(64);
        let payload = b"partial payload".to_vec();

        let write_task = tokio::spawn(async move {
            write_frame(&mut writer, &payload).await.unwrap();
        });
        let decoded = read_frame(&mut reader).await.unwrap();

        write_task.await.unwrap();
        assert_eq!(decoded, b"partial payload");
    }

    #[tokio::test]
    async fn client_and_server_handshake_exchange_peer_identity() {
        let local = DeviceInfo::new_local("Windows Desk", 45731);
        let remote = DeviceInfo::new_local("MacBook", 45731);
        let expected_remote = remote.id.clone();
        let (mut client, mut server) = duplex(4096);

        let server_task = tokio::spawn({
            let local = local.clone();
            async move {
                server_handshake(&mut server, local, |device| device.id == expected_remote)
                    .await
                    .unwrap()
            }
        });
        let client_peer = client_handshake(&mut client, remote.clone(), local.id.clone())
            .await
            .unwrap();
        let server_peer = server_task.await.unwrap();

        assert_eq!(client_peer, local);
        assert_eq!(server_peer, remote);
    }

    #[tokio::test]
    async fn server_rejects_unpaired_peer() {
        let local = DeviceInfo::new_local("Windows Desk", 45731);
        let remote = DeviceInfo::new_local("Unknown", 45731);
        let (mut client, mut server) = duplex(4096);

        let server_task = tokio::spawn({
            let local = local.clone();
            async move {
                server_handshake(&mut server, local, |_| false)
                    .await
                    .unwrap_err()
            }
        });
        let client_error = client_handshake(&mut client, remote, local.id.clone())
            .await
            .unwrap_err();
        let server_error = server_task.await.unwrap();

        assert!(matches!(client_error, TransportError::HandshakeRejected(_)));
        assert!(matches!(server_error, TransportError::UnpairedPeer));
    }

    #[tokio::test]
    async fn heartbeat_messages_round_trip_as_control_frames() {
        let (mut writer, mut reader) = duplex(4096);
        let ping = TransportMessage::Ping { nonce: 42 };
        let expected = ping.clone();

        let write_task = tokio::spawn(async move {
            write_message(&mut writer, &ping).await.unwrap();
        });
        let message = read_message(&mut reader).await.unwrap();

        write_task.await.unwrap();
        assert_eq!(message, expected);
    }

    #[tokio::test]
    async fn preferred_tcp_port_falls_back_when_occupied() {
        let occupied = TcpListener::bind(("0.0.0.0", 0)).await.unwrap();
        let occupied_port = occupied.local_addr().unwrap().port();

        let (listener, actual_port, used_fallback) =
            bind_transport_listener(occupied_port).await.unwrap();

        assert!(used_fallback);
        assert_ne!(actual_port, occupied_port);
        assert_eq!(listener.local_addr().unwrap().port(), actual_port);
    }

    #[tokio::test]
    async fn unpair_message_reaches_the_connected_peer() {
        let local_a = DeviceInfo::new_local("Device A", 45731);
        let local_b = DeviceInfo::new_local("Device B", 45731);
        let registry_a = Arc::new(Mutex::new(PeerRegistry::from_paired(vec![PairedPeer {
            device: local_b.clone(),
            receive_clipboard: true,
            send_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        }])));
        let registry_b = Arc::new(Mutex::new(PeerRegistry::from_paired(vec![PairedPeer {
            device: local_a.clone(),
            receive_clipboard: true,
            send_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        }])));
        let (runtime_a, _events_a) = TransportRuntime::new(local_a.clone(), registry_a);
        let (runtime_b, mut events_b) = TransportRuntime::new(local_b.clone(), registry_b);

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn({
            let runtime_b = runtime_b.clone();
            async move {
                let (stream, _) = listener.accept().await.unwrap();
                let _ = runtime_b.accept_connection(stream).await;
            }
        });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let peer = client_handshake(&mut stream, local_a.clone(), local_b.id.clone())
            .await
            .unwrap();
        tokio::spawn({
            let runtime_a = runtime_a.clone();
            async move {
                let _ = runtime_a.run_connection(stream, peer).await;
            }
        });

        for _ in 0..100 {
            if runtime_a.is_connected(&local_b.id) {
                break;
            }
            time::sleep(Duration::from_millis(10)).await;
        }
        assert!(runtime_a.is_connected(&local_b.id));

        runtime_a
            .send_message(&local_b.id, TransportMessage::Unpair)
            .await
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let event = time::timeout(remaining, events_b.recv())
                .await
                .expect("did not receive the unpair notice")
                .unwrap();
            if matches!(
                event,
                TransportEvent::Message {
                    message: TransportMessage::Unpair,
                    ..
                }
            ) {
                break;
            }
        }
    }

    #[test]
    fn disconnect_peer_removes_connection_and_marks_peer_offline() {
        let local = DeviceInfo::new_local("Windows Desk", 45731);
        let peer = DeviceInfo::new_local("MacBook", 45731);
        let peer_id = peer.id.clone();
        let registry = Arc::new(Mutex::new(PeerRegistry::from_paired(vec![PairedPeer {
            device: peer.clone(),
            receive_clipboard: true,
            send_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        }])));
        let (runtime, mut events) = TransportRuntime::new(local, registry.clone());
        let (sender, _receiver) = mpsc::channel(1);
        runtime.connections.lock().unwrap().insert(
            peer_id.clone(),
            ConnectionEntry {
                token: "test-token".to_string(),
                sender,
            },
        );

        runtime.disconnect_peer(&peer_id);

        assert!(!runtime.is_connected(&peer_id));
        assert_eq!(
            registry.lock().unwrap().paired()[0].state,
            PeerConnectionState::Offline
        );
        assert!(matches!(
            events.try_recv(),
            Ok(TransportEvent::PeerDisconnected { reason_code, .. })
                if reason_code == "unpaired_peer"
        ));
    }
}
