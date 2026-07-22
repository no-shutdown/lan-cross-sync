use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub Uuid);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub app_version: String,
    pub protocol_version: u16,
    pub port: u16,
    pub capabilities: Vec<Capability>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Discovery,
    Pairing,
    Clipboard,
    FileTransfer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerConnectionState {
    Offline,
    Discovered,
    Pairing,
    Connected,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PairedPeer {
    pub device: DeviceInfo,
    pub receive_clipboard: bool,
    pub is_default_file_target: bool,
    pub state: PeerConnectionState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalSettings {
    pub local_device: DeviceInfo,
    pub paired_peers: Vec<PairedPeer>,
    pub autostart_enabled: bool,
}

impl DeviceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl DeviceInfo {
    pub fn new_local(name: impl Into<String>, port: u16) -> Self {
        Self {
            id: DeviceId::new(),
            name: name.into(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: 1,
            port,
            capabilities: vec![
                Capability::Discovery,
                Capability::Pairing,
                Capability::Clipboard,
                Capability::FileTransfer,
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_device_has_expected_capabilities() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);

        assert_eq!(device.name, "Windows Desk");
        assert_eq!(device.protocol_version, 1);
        assert_eq!(device.port, 45731);
        assert!(device.capabilities.contains(&Capability::Discovery));
        assert!(device.capabilities.contains(&Capability::Pairing));
        assert!(device.capabilities.contains(&Capability::Clipboard));
        assert!(device.capabilities.contains(&Capability::FileTransfer));
    }

    #[test]
    fn paired_peer_defaults_can_be_serialized() {
        let peer = PairedPeer {
            device: DeviceInfo::new_local("MacBook", 45731),
            receive_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Offline,
        };

        let json = serde_json::to_string(&peer).unwrap();
        let decoded: PairedPeer = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, peer);
    }
}
