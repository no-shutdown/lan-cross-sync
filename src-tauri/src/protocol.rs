use crate::domain::{DeviceId, DeviceInfo};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LanMessage {
    Discovery(DiscoveryPacket),
    PairingRequest(PairingRequest),
    PairingResponse(PairingResponse),
    PairingConfirm(PairingConfirm),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryPacket {
    pub protocol_version: u16,
    pub device: DeviceInfo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PairingRequest {
    pub protocol_version: u16,
    pub request_id: String,
    pub target_device_id: DeviceId,
    pub from_device: DeviceInfo,
    pub code: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PairingResponse {
    pub protocol_version: u16,
    pub request_id: String,
    pub session_id: String,
    pub accepted: bool,
    pub from_device: DeviceInfo,
    pub reason_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PairingConfirm {
    pub protocol_version: u16,
    pub request_id: String,
    pub session_id: String,
    pub target_device_id: DeviceId,
    pub from_device_id: DeviceId,
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
    pub fn accepted(
        request_id: impl Into<String>,
        session_id: impl Into<String>,
        from_device: DeviceInfo,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            session_id: session_id.into(),
            accepted: true,
            from_device,
            reason_code: None,
        }
    }

    pub fn rejected(
        request_id: impl Into<String>,
        session_id: impl Into<String>,
        from_device: DeviceInfo,
        reason_code: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            session_id: session_id.into(),
            accepted: false,
            from_device,
            reason_code: Some(reason_code.into()),
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
            "request-1",
            "session-1",
            device,
            "invalid_code",
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
        assert!(json["device"].is_object());
        let device_id = json["device"]["id"].as_str().unwrap();
        uuid::Uuid::parse_str(device_id).unwrap();
        assert!(json["device"]["app_version"].is_string());
        assert!(json["device"].get("appVersion").is_none());
        assert!(json["device"].get("protocolVersion").is_none());
        assert_eq!(json["device"]["protocol_version"], PROTOCOL_VERSION);
    }

    #[test]
    fn pairing_response_json_has_wire_shape() {
        let device = DeviceInfo::new_local("Windows Desk", 45731);
        let response = LanMessage::PairingResponse(PairingResponse::rejected(
            "request-1",
            "session-1",
            device,
            "invalid_code",
        ));

        let encoded = encode_message(&response).unwrap();
        let json: Value = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(json["type"], "pairing_response");
        assert_eq!(json["protocol_version"], PROTOCOL_VERSION);
        assert_eq!(json["request_id"], "request-1");
        assert_eq!(json["session_id"], "session-1");
        assert_eq!(json["accepted"], false);
        assert!(json["from_device"].is_object());
        assert_eq!(json["reason_code"], "invalid_code");
        assert!(json.get("from_device_id").is_none());
        assert!(json.get("reason").is_none());
        assert!(json.get("protocolVersion").is_none());
        assert!(json.get("sessionId").is_none());
    }

    #[test]
    fn pairing_request_json_has_target_and_code() {
        let sender = DeviceInfo::new_local("MacBook", 45731);
        let target = DeviceInfo::new_local("Windows Desk", 45731);
        let message = LanMessage::PairingRequest(PairingRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-1".to_string(),
            target_device_id: target.id,
            from_device: sender,
            code: "123456".to_string(),
        });

        let json: Value = serde_json::from_slice(&encode_message(&message).unwrap()).unwrap();

        assert_eq!(json["type"], "pairing_request");
        assert_eq!(json["request_id"], "request-1");
        assert_eq!(json["code"], "123456");
        assert!(json["target_device_id"].is_string());
        assert!(json["from_device"].is_object());
    }

    #[test]
    fn pairing_confirm_round_trips() {
        let target = DeviceInfo::new_local("Windows Desk", 45731);
        let from = DeviceInfo::new_local("MacBook", 45731);
        let message = LanMessage::PairingConfirm(PairingConfirm {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-1".to_string(),
            session_id: "session-1".to_string(),
            target_device_id: target.id,
            from_device_id: from.id,
        });

        assert_eq!(
            decode_message(&encode_message(&message).unwrap()).unwrap(),
            message
        );
    }

    #[test]
    fn business_protocol_starts_new_version() {
        assert_eq!(PROTOCOL_VERSION, 2);
    }
}
