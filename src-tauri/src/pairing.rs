use crate::{
    domain::{DeviceInfo, LocalSettings, PairedPeer, PeerConnectionState},
    registry::PeerRegistry,
    settings::SettingsStore,
};
use rand::RngExt;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const PAIRING_CODE_TTL: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct PendingOutgoing {
    pub peer: DeviceInfo,
    created_at: Instant,
}

#[derive(Clone, Debug)]
pub struct PendingIncoming {
    pub peer: DeviceInfo,
    pub session_id: String,
    created_at: Instant,
}

#[derive(Clone, Debug, Default)]
pub struct PairingRuntimeState {
    outgoing: HashMap<String, PendingOutgoing>,
    incoming: HashMap<String, PendingIncoming>,
}

impl PairingRuntimeState {
    pub fn register_outgoing(&mut self, request_id: impl Into<String>, peer: DeviceInfo) {
        self.outgoing.insert(
            request_id.into(),
            PendingOutgoing {
                peer,
                created_at: Instant::now(),
            },
        );
    }

    pub fn take_outgoing(&mut self, request_id: &str) -> Option<PendingOutgoing> {
        let pending = self.outgoing.remove(request_id)?;
        (!pending.created_at.elapsed().gt(&PAIRING_CODE_TTL)).then_some(pending)
    }

    pub fn register_incoming(
        &mut self,
        request_id: impl Into<String>,
        session_id: impl Into<String>,
        peer: DeviceInfo,
    ) {
        self.incoming.insert(
            request_id.into(),
            PendingIncoming {
                peer,
                session_id: session_id.into(),
                created_at: Instant::now(),
            },
        );
    }

    pub fn take_incoming(&mut self, request_id: &str) -> Option<PendingIncoming> {
        let pending = self.incoming.remove(request_id)?;
        (!pending.created_at.elapsed().gt(&PAIRING_CODE_TTL)).then_some(pending)
    }

    pub fn clear_expired(&mut self) {
        self.outgoing
            .retain(|_, pending| pending.created_at.elapsed() <= PAIRING_CODE_TTL);
        self.incoming
            .retain(|_, pending| pending.created_at.elapsed() <= PAIRING_CODE_TTL);
    }
}

#[derive(Clone)]
pub struct PairingRuntime {
    pub local_device: DeviceInfo,
    pub settings: Arc<Mutex<LocalSettings>>,
    pub settings_store: SettingsStore,
    pub registry: Arc<Mutex<PeerRegistry>>,
    pub active: Arc<Mutex<Option<PairingSession>>>,
    pub requests: Arc<Mutex<PairingRuntimeState>>,
    pub last_error: Arc<Mutex<Option<String>>>,
}

impl PairingRuntime {
    pub fn new(
        local_device: DeviceInfo,
        settings: Arc<Mutex<LocalSettings>>,
        settings_store: SettingsStore,
        registry: Arc<Mutex<PeerRegistry>>,
        active: Arc<Mutex<Option<PairingSession>>>,
    ) -> Self {
        Self {
            local_device,
            settings,
            settings_store,
            registry,
            active,
            requests: Arc::new(Mutex::new(PairingRuntimeState::default())),
            last_error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn clear_error(&self) {
        *self.last_error.lock().unwrap() = None;
    }

    pub fn record_error(&self, reason_code: impl Into<String>) {
        *self.last_error.lock().unwrap() = Some(reason_code.into());
    }

    pub fn persist_peer(&self, device: DeviceInfo, endpoint: SocketAddr) -> anyhow::Result<()> {
        let mut settings = self.settings.lock().unwrap();
        let existing = settings
            .paired_peers
            .iter()
            .find(|peer| peer.device.id == device.id)
            .cloned();
        let has_default_target = settings
            .paired_peers
            .iter()
            .any(|peer| peer.is_default_file_target);
        let peer = PairedPeer {
            device: device.clone(),
            receive_clipboard: existing
                .as_ref()
                .map(|peer| peer.receive_clipboard)
                .unwrap_or(true),
            is_default_file_target: existing
                .as_ref()
                .map(|peer| peer.is_default_file_target)
                .unwrap_or(!has_default_target),
            state: PeerConnectionState::Discovered,
        };
        let mut next = settings.clone();
        next.paired_peers.retain(|item| item.device.id != device.id);
        next.paired_peers.push(peer.clone());
        self.settings_store.save(&next)?;
        *settings = next;
        drop(settings);

        let mut registry = self.registry.lock().unwrap();
        registry.mark_discovered_at(device, endpoint);
        registry.set_paired(peer);
        Ok(())
    }
}

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

    #[test]
    fn pairing_runtime_tracks_each_request_once() {
        let peer = DeviceInfo::new_local("MacBook", 45731);
        let mut state = PairingRuntimeState::default();

        state.register_outgoing("request-1", peer.clone());

        assert_eq!(state.take_outgoing("request-1").unwrap().peer, peer);
        assert!(state.take_outgoing("request-1").is_none());
    }

    #[test]
    fn pairing_runtime_tracks_pending_confirmation() {
        let peer = DeviceInfo::new_local("MacBook", 45731);
        let mut state = PairingRuntimeState::default();

        state.register_incoming("request-1", "session-1", peer.clone());

        let pending = state.take_incoming("request-1").unwrap();
        assert_eq!(pending.session_id, "session-1");
        assert_eq!(pending.peer, peer);
    }
}
