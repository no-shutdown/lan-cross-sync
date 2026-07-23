use crate::{
    domain::{DeviceId, DeviceInfo},
    pairing::PairingRuntime,
    protocol::{
        decode_message, encode_message, DiscoveryPacket, LanMessage, PairingConfirm,
        PairingRequest, PairingResponse, PROTOCOL_VERSION,
    },
    registry::PeerRegistry,
};
use anyhow::{Context, Result};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{Arc, Mutex},
};
use tokio::{
    net::UdpSocket,
    time::{self, Duration},
};

pub const DISCOVERY_BROADCAST_ADDR: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);
pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(3);

pub fn discovery_socket_addr(port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(DISCOVERY_BROADCAST_ADDR, port)
}

pub fn encode_discovery(device: DeviceInfo) -> Result<Vec<u8>> {
    let message = LanMessage::Discovery(DiscoveryPacket::new(device));
    encode_message(&message).context("failed to encode discovery packet")
}

pub fn decode_discovery(bytes: &[u8]) -> Result<Option<DeviceInfo>> {
    match decode_message(bytes).context("failed to decode LAN message")? {
        LanMessage::Discovery(packet)
            if packet.protocol_version == PROTOCOL_VERSION
                && packet.device.protocol_version == PROTOCOL_VERSION =>
        {
            Ok(Some(packet.device))
        }
        LanMessage::Discovery(_) => Ok(None),
        _ => Ok(None),
    }
}

pub fn apply_discovery_packet(
    bytes: &[u8],
    local_device_id: &DeviceId,
    registry: &mut PeerRegistry,
) -> Result<bool> {
    apply_discovery_packet_without_endpoint(bytes, local_device_id, registry)
}

pub fn apply_discovery_packet_at(
    bytes: &[u8],
    local_device_id: &DeviceId,
    source: SocketAddr,
    registry: &mut PeerRegistry,
) -> Result<bool> {
    let Some(device) = decode_discovery(bytes)? else {
        return Ok(false);
    };

    if device.id == *local_device_id {
        return Ok(false);
    }

    registry.mark_discovered_at(device, source);
    Ok(true)
}

fn apply_discovery_packet_without_endpoint(
    bytes: &[u8],
    local_device_id: &DeviceId,
    registry: &mut PeerRegistry,
) -> Result<bool> {
    let Some(device) = decode_discovery(bytes)? else {
        return Ok(false);
    };

    if device.id == *local_device_id {
        return Ok(false);
    }

    registry.mark_discovered(device);
    Ok(true)
}

pub async fn announce_loop(device: DeviceInfo, port: u16) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0))
        .await
        .context("failed to bind discovery UDP socket")?;
    socket
        .set_broadcast(true)
        .context("failed to enable discovery UDP broadcast")?;
    let target = discovery_socket_addr(port);
    let payload = encode_discovery(device)?;
    let mut interval = time::interval(DISCOVERY_INTERVAL);

    loop {
        interval.tick().await;
        socket
            .send_to(&payload, target)
            .await
            .with_context(|| format!("failed to send discovery packet to {target}"))?;
    }
}

pub async fn receive_loop(
    local_device_id: DeviceId,
    port: u16,
    registry: Arc<Mutex<PeerRegistry>>,
) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("failed to bind discovery UDP listener on port {port}"))?;
    let mut buffer = vec![0_u8; 64 * 1024];

    loop {
        let (len, source) = socket
            .recv_from(&mut buffer)
            .await
            .context("failed to receive discovery UDP packet")?;
        let packet = &buffer[..len];

        match registry.lock() {
            Ok(mut registry) => {
                if let Err(err) =
                    apply_discovery_packet_at(packet, &local_device_id, source, &mut registry)
                {
                    tracing::debug!(?err, %source, "ignored invalid discovery packet");
                }
            }
            Err(err) => {
                tracing::error!(?err, "discovery registry lock poisoned");
                return Ok(());
            }
        }
    }
}

pub async fn receive_loop_with_pairing(port: u16, pairing: Arc<PairingRuntime>) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("failed to bind discovery UDP listener on port {port}"))?;
    let mut buffer = vec![0_u8; 64 * 1024];

    loop {
        let (len, source) = socket
            .recv_from(&mut buffer)
            .await
            .context("failed to receive LAN message")?;
        if let Err(err) = handle_lan_message(&socket, &buffer[..len], source, &pairing).await {
            tracing::debug!(?err, %source, "ignored invalid LAN message");
        }
    }
}

async fn handle_lan_message(
    socket: &UdpSocket,
    bytes: &[u8],
    source: SocketAddr,
    pairing: &PairingRuntime,
) -> Result<()> {
    let message = decode_message(bytes).context("failed to decode LAN message")?;
    match message {
        LanMessage::Discovery(packet)
            if packet.protocol_version == PROTOCOL_VERSION
                && packet.device.protocol_version == PROTOCOL_VERSION =>
        {
            let mut registry = pairing.registry.lock().unwrap();
            if packet.device.id != pairing.local_device.id {
                registry.mark_discovered_at(packet.device, source);
            }
        }
        LanMessage::Discovery(_) => {}
        LanMessage::PairingRequest(request) => {
            handle_pairing_request(socket, request, source, pairing).await?;
        }
        LanMessage::PairingResponse(response) => {
            handle_pairing_response(socket, response, source, pairing).await?;
        }
        LanMessage::PairingConfirm(confirm) => {
            handle_pairing_confirm(confirm, source, pairing)?;
        }
    }

    Ok(())
}

async fn handle_pairing_request(
    socket: &UdpSocket,
    request: PairingRequest,
    source: SocketAddr,
    pairing: &PairingRuntime,
) -> Result<()> {
    if request.target_device_id != pairing.local_device.id {
        return Ok(());
    }

    let response = if request.from_device.id == pairing.local_device.id {
        PairingResponse::rejected(
            request.request_id,
            String::new(),
            pairing.local_device.clone(),
            "self_device",
        )
    } else if request.protocol_version != PROTOCOL_VERSION
        || request.from_device.protocol_version != PROTOCOL_VERSION
    {
        PairingResponse::rejected(
            request.request_id,
            String::new(),
            pairing.local_device.clone(),
            "unsupported_protocol",
        )
    } else {
        let mut active = pairing.active.lock().unwrap();
        let session_state = active.as_ref().map(|session| {
            (
                session.session_id.clone(),
                session.is_expired(),
                session.verify_code(&request.code),
            )
        });

        match session_state {
            None => {
                pairing.record_error("no_active_pairing");
                PairingResponse::rejected(
                    request.request_id,
                    String::new(),
                    pairing.local_device.clone(),
                    "no_active_pairing",
                )
            }
            Some((session_id, expired, _)) if expired => {
                *active = None;
                PairingResponse::rejected(
                    request.request_id,
                    session_id,
                    pairing.local_device.clone(),
                    "expired_code",
                )
            }
            Some((session_id, _, false)) => PairingResponse::rejected(
                request.request_id,
                session_id,
                pairing.local_device.clone(),
                "invalid_code",
            ),
            Some((session_id, _, true)) => {
                pairing.requests.lock().unwrap().register_incoming(
                    request.request_id.clone(),
                    session_id.clone(),
                    request.from_device.clone(),
                );
                PairingResponse::accepted(
                    request.request_id,
                    session_id,
                    pairing.local_device.clone(),
                )
            }
        }
    };

    send_pairing_response(socket, response, source).await
}

async fn send_pairing_response(
    socket: &UdpSocket,
    response: PairingResponse,
    target: SocketAddr,
) -> Result<()> {
    let bytes = encode_message(&LanMessage::PairingResponse(response))
        .context("failed to encode pairing response")?;
    socket
        .send_to(&bytes, target)
        .await
        .with_context(|| format!("failed to send pairing response to {target}"))?;
    Ok(())
}

async fn handle_pairing_response(
    socket: &UdpSocket,
    response: PairingResponse,
    source: SocketAddr,
    pairing: &PairingRuntime,
) -> Result<()> {
    let Some(pending) = pairing
        .requests
        .lock()
        .unwrap()
        .take_outgoing(&response.request_id)
    else {
        return Ok(());
    };

    if !response.accepted {
        pairing.record_error(
            response
                .reason_code
                .unwrap_or_else(|| "pairing_rejected".to_string()),
        );
        return Ok(());
    }

    if response.from_device.id != pending.peer.id
        || response.session_id.is_empty()
        || response.from_device.id == pairing.local_device.id
    {
        pairing.record_error("invalid_pairing_response");
        return Ok(());
    }

    pairing.persist_peer(response.from_device.clone(), source)?;
    let confirm = PairingConfirm {
        protocol_version: PROTOCOL_VERSION,
        request_id: response.request_id,
        session_id: response.session_id,
        target_device_id: response.from_device.id,
        from_device_id: pairing.local_device.id.clone(),
    };
    let bytes = encode_message(&LanMessage::PairingConfirm(confirm))
        .context("failed to encode pairing confirmation")?;
    socket
        .send_to(&bytes, source)
        .await
        .with_context(|| format!("failed to send pairing confirmation to {source}"))?;
    pairing.clear_error();
    Ok(())
}

fn handle_pairing_confirm(
    confirm: PairingConfirm,
    source: SocketAddr,
    pairing: &PairingRuntime,
) -> Result<()> {
    if confirm.target_device_id != pairing.local_device.id
        || confirm.from_device_id == pairing.local_device.id
    {
        return Ok(());
    }

    let Some(pending) = pairing
        .requests
        .lock()
        .unwrap()
        .take_incoming(&confirm.request_id)
    else {
        return Ok(());
    };

    if pending.session_id != confirm.session_id || pending.peer.id != confirm.from_device_id {
        pairing.record_error("invalid_pairing_confirmation");
        return Ok(());
    }

    pairing.persist_peer(pending.peer, source)?;
    *pairing.active.lock().unwrap() = None;
    pairing.clear_error();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PairedPeer, PeerConnectionState};
    use std::net::SocketAddr;

    #[test]
    fn discovery_packet_round_trips_to_device_info() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let encoded = encode_discovery(device.clone()).unwrap();
        let decoded = decode_discovery(&encoded).unwrap();

        assert_eq!(decoded, Some(device));
    }

    #[test]
    fn broadcast_address_uses_selected_port() {
        let addr = discovery_socket_addr(45731);

        assert_eq!(*addr.ip(), DISCOVERY_BROADCAST_ADDR);
        assert_eq!(addr.port(), 45731);
    }

    #[test]
    fn decode_discovery_ignores_unsupported_packet_version() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let message = LanMessage::Discovery(DiscoveryPacket {
            protocol_version: PROTOCOL_VERSION + 1,
            device,
        });
        let encoded = encode_message(&message).unwrap();
        let decoded = decode_discovery(&encoded).unwrap();

        assert_eq!(decoded, None);
    }

    #[test]
    fn decode_discovery_ignores_unsupported_device_version() {
        let mut device = DeviceInfo::new_local("Windows Desk", 45731);
        device.protocol_version = PROTOCOL_VERSION + 1;
        let encoded = encode_discovery(device).unwrap();
        let decoded = decode_discovery(&encoded).unwrap();

        assert_eq!(decoded, None);
    }

    #[test]
    fn decode_discovery_ignores_non_discovery_messages() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let message = LanMessage::PairingRequest(crate::protocol::PairingRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request".to_string(),
            target_device_id: device.id.clone(),
            from_device: device,
            code: "123456".to_string(),
        });
        let encoded = encode_message(&message).unwrap();
        let decoded = decode_discovery(&encoded).unwrap();

        assert_eq!(decoded, None);
    }

    #[test]
    fn apply_discovery_packet_adds_remote_device() {
        let local = DeviceInfo::new_local("Windows Desk", 45731);
        let remote = DeviceInfo::new_local("MacBook", 45731);
        let encoded = encode_discovery(remote.clone()).unwrap();
        let mut registry = PeerRegistry::new();

        let applied = apply_discovery_packet(&encoded, &local.id, &mut registry).unwrap();

        assert!(applied);
        assert_eq!(registry.discovered(), vec![remote]);
    }

    #[test]
    fn apply_discovery_packet_ignores_local_device() {
        let local = DeviceInfo::new_local("Windows Desk", 45731);
        let encoded = encode_discovery(local.clone()).unwrap();
        let mut registry = PeerRegistry::new();

        let applied = apply_discovery_packet(&encoded, &local.id, &mut registry).unwrap();

        assert!(!applied);
        assert!(registry.discovered().is_empty());
    }

    #[test]
    fn apply_discovery_packet_at_records_source_endpoint() {
        let local = DeviceInfo::new_local("Windows Desk", 45731);
        let remote = DeviceInfo::new_local("MacBook", 45731);
        let encoded = encode_discovery(remote.clone()).unwrap();
        let source: SocketAddr = "192.168.1.20:45731".parse().unwrap();
        let mut registry = PeerRegistry::new();

        let applied =
            apply_discovery_packet_at(&encoded, &local.id, source, &mut registry).unwrap();

        assert!(applied);
        assert_eq!(registry.endpoint(&remote.id), Some(source));
    }

    #[test]
    fn apply_discovery_packet_updates_paired_device_state() {
        let local = DeviceInfo::new_local("Windows Desk", 45731);
        let remote = DeviceInfo::new_local("MacBook", 45731);
        let encoded = encode_discovery(remote.clone()).unwrap();
        let mut registry = PeerRegistry::from_paired(vec![PairedPeer {
            device: remote,
            receive_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Offline,
        }]);

        let applied = apply_discovery_packet(&encoded, &local.id, &mut registry).unwrap();

        assert!(applied);
        assert!(registry.discovered().is_empty());
        assert_eq!(registry.paired()[0].state, PeerConnectionState::Discovered);
    }
}
