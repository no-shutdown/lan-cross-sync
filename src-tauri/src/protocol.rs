use crate::domain::{DeviceId, DeviceInfo};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LanMessage {
    Discovery(DiscoveryPacket),
    PairingRequest(PairingRequest),
    PairingResponse(PairingResponse),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryPacket {
    pub protocol_version: u16,
    pub device: DeviceInfo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PairingRequest {
    pub protocol_version: u16,
    pub session_id: String,
    pub from_device: DeviceInfo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PairingResponse {
    pub protocol_version: u16,
    pub session_id: String,
    pub accepted: bool,
    pub from_device_id: DeviceId,
    pub reason: Option<String>,
}

impl DiscoveryPacket {
    pub fn new(device: DeviceInfo) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            device,
        }
    }
}

impl PairingResponse {
    pub fn rejected(
        session_id: impl Into<String>,
        from_device_id: DeviceId,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            session_id: session_id.into(),
            accepted: false,
            from_device_id,
            reason: Some(reason.into()),
        }
    }
}

pub fn encode_message(message: &LanMessage) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(message)
}

pub fn decode_message(bytes: &[u8]) -> Result<LanMessage, serde_json::Error> {
    serde_json::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DeviceInfo;
    use serde_json::Value;

    #[test]
    fn discovery_packet_round_trips() {
        let device = DeviceInfo::new_local("MacBook", 45731);
        let message = LanMessage::Discovery(DiscoveryPacket::new(device.clone()));

        let encoded = encode_message(&message).unwrap();
        let decoded = decode_message(&encoded).unwrap();

        assert_eq!(decoded, message);
    }

    #[test]
    fn pairing_response_can_reject_with_reason() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let response = LanMessage::PairingResponse(PairingResponse::rejected(
            "abc123",
            device.id,
            "invalid code",
        ));

        let encoded = encode_message(&response).unwrap();
        let decoded = decode_message(&encoded).unwrap();

        assert_eq!(decoded, response);
    }

    #[test]
    fn discovery_json_has_wire_shape() {
        let device = DeviceInfo::new_local("MacBook", 45731);
        let message = LanMessage::Discovery(DiscoveryPacket::new(device));

        let encoded = encode_message(&message).unwrap();
        let json: Value = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(json["type"], "discovery");
        assert_eq!(json["protocol_version"], PROTOCOL_VERSION);
    }

    #[test]
    fn pairing_response_json_has_wire_shape() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let response = LanMessage::PairingResponse(PairingResponse::rejected(
            "abc123",
            device.id,
            "invalid code",
        ));

        let encoded = encode_message(&response).unwrap();
        let json: Value = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(json["type"], "pairing_response");
        assert_eq!(json["protocol_version"], PROTOCOL_VERSION);
        assert_eq!(json["session_id"], "abc123");
        assert_eq!(json["accepted"], false);
        assert!(json["from_device_id"].is_string());
        assert_eq!(json["reason"], "invalid code");
        assert!(json.get("fromDeviceId").is_none());
        assert!(json.get("protocolVersion").is_none());
        assert!(json.get("sessionId").is_none());
    }
}
