use crate::{
    domain::DeviceInfo,
    protocol::{decode_message, encode_message, DiscoveryPacket, LanMessage},
};
use anyhow::{Context, Result};
use std::net::{Ipv4Addr, SocketAddrV4};
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
        LanMessage::Discovery(packet) => Ok(Some(packet.device)),
        _ => Ok(None),
    }
}

pub async fn announce_loop(device: DeviceInfo, port: u16) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    socket.set_broadcast(true)?;
    let target = discovery_socket_addr(port);
    let payload = encode_discovery(device)?;
    let mut interval = time::interval(DISCOVERY_INTERVAL);

    loop {
        interval.tick().await;
        socket.send_to(&payload, target).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
