use crate::domain::DeviceInfo;
use rand::RngExt;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const PAIRING_CODE_TTL: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct PairingSession {
    pub session_id: String,
    pub code: String,
    pub local_device: DeviceInfo,
    created_at: Instant,
}

impl PairingSession {
    pub fn new(local_device: DeviceInfo) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            code: generate_pairing_code(),
            local_device,
            created_at: Instant::now(),
        }
    }

    pub fn with_code_for_test(local_device: DeviceInfo, code: impl Into<String>) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            code: code.into(),
            local_device,
            created_at: Instant::now(),
        }
    }

    #[cfg(test)]
    pub fn expired_for_test(local_device: DeviceInfo, code: impl Into<String>) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            code: code.into(),
            local_device,
            created_at: Instant::now() - PAIRING_CODE_TTL - Duration::from_secs(1),
        }
    }

    pub fn verify_code(&self, candidate: &str) -> bool {
        !self.is_expired() && self.code == candidate
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > PAIRING_CODE_TTL
    }
}

pub fn generate_pairing_code() -> String {
    let code = rand::rng().random_range(0..=999_999);
    format!("{code:06}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_code_is_six_digits() {
        let code = generate_pairing_code();

        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn session_accepts_matching_code() {
        let device = DeviceInfo::new_local("MacBook", 45731);
        let session = PairingSession::with_code_for_test(device, "123456");

        assert!(session.verify_code("123456"));
        assert!(!session.verify_code("654321"));
    }

    #[test]
    fn expired_session_rejects_matching_code() {
        let device = DeviceInfo::new_local("MacBook", 45731);
        let session = PairingSession::expired_for_test(device, "123456");

        assert!(session.is_expired());
        assert!(!session.verify_code("123456"));
    }
}
