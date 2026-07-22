use crate::domain::{DeviceId, DeviceInfo, PairedPeer, PeerConnectionState};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct PeerRegistry {
    discovered: HashMap<DeviceId, DeviceInfo>,
    paired: HashMap<DeviceId, PairedPeer>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_paired(peers: impl IntoIterator<Item = PairedPeer>) -> Self {
        let paired = peers
            .into_iter()
            .map(|peer| (peer.device.id.clone(), peer))
            .collect();
        Self {
            discovered: HashMap::new(),
            paired,
        }
    }

    pub fn mark_discovered(&mut self, device: DeviceInfo) {
        if !self.paired.contains_key(&device.id) {
            self.discovered.insert(device.id.clone(), device);
        } else if let Some(peer) = self.paired.get_mut(&device.id) {
            peer.device = device;
            if matches!(
                peer.state,
                PeerConnectionState::Offline | PeerConnectionState::Error
            ) {
                peer.state = PeerConnectionState::Discovered;
            }
        }
    }

    pub fn set_paired(&mut self, peer: PairedPeer) {
        self.discovered.remove(&peer.device.id);
        self.paired.insert(peer.device.id.clone(), peer);
    }

    pub fn set_state(&mut self, id: &DeviceId, state: PeerConnectionState) {
        if let Some(peer) = self.paired.get_mut(id) {
            peer.state = state;
        }
    }

    pub fn sync_preferences(&mut self, peers: &[PairedPeer]) {
        for peer in peers {
            if let Some(current) = self.paired.get_mut(&peer.device.id) {
                current.receive_clipboard = peer.receive_clipboard;
                current.is_default_file_target = peer.is_default_file_target;
            } else {
                self.set_paired(peer.clone());
            }
        }
    }

    pub fn remove_pairing(&mut self, id: &DeviceId) {
        self.paired.remove(id);
        self.discovered.remove(id);
    }

    pub fn discovered(&self) -> Vec<DeviceInfo> {
        self.discovered.values().cloned().collect()
    }

    pub fn paired(&self) -> Vec<PairedPeer> {
        self.paired.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_device_moves_to_paired() {
        let mut registry = PeerRegistry::new();
        let device = DeviceInfo::new_local("MacBook", 45731);

        registry.mark_discovered(device.clone());
        assert_eq!(registry.discovered().len(), 1);

        registry.set_paired(PairedPeer {
            device,
            receive_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        });

        assert!(registry.discovered().is_empty());
        assert_eq!(registry.paired().len(), 1);
        assert_eq!(registry.paired()[0].state, PeerConnectionState::Connected);
    }

    #[test]
    fn paired_device_discovery_updates_state() {
        let mut registry = PeerRegistry::new();
        let device = DeviceInfo::new_local("MacBook", 45731);
        registry.set_paired(PairedPeer {
            device: device.clone(),
            receive_clipboard: false,
            is_default_file_target: false,
            state: PeerConnectionState::Offline,
        });

        registry.mark_discovered(device);

        assert_eq!(registry.paired()[0].state, PeerConnectionState::Discovered);
    }

    #[test]
    fn paired_connected_device_stays_connected_when_rediscovered() {
        let mut registry = PeerRegistry::new();
        let device = DeviceInfo::new_local("MacBook", 45731);
        registry.set_paired(PairedPeer {
            device: device.clone(),
            receive_clipboard: false,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        });

        registry.mark_discovered(device);

        assert_eq!(registry.paired()[0].state, PeerConnectionState::Connected);
    }

    #[test]
    fn rediscovery_updates_device_metadata_and_preserves_preferences() {
        let mut registry = PeerRegistry::new();
        let device = DeviceInfo::new_local("MacBook", 45731);
        registry.set_paired(PairedPeer {
            device: device.clone(),
            receive_clipboard: true,
            is_default_file_target: true,
            state: PeerConnectionState::Connected,
        });

        let mut updated_device = device;
        updated_device.name = "MacBook Pro".to_string();
        updated_device.port = 45732;
        registry.mark_discovered(updated_device);

        let peer = &registry.paired()[0];
        assert_eq!(peer.device.name, "MacBook Pro");
        assert_eq!(peer.device.port, 45732);
        assert!(peer.receive_clipboard);
        assert!(peer.is_default_file_target);
        assert_eq!(peer.state, PeerConnectionState::Connected);
    }

    #[test]
    fn from_paired_hydrates_paired_devices() {
        let first = PairedPeer {
            device: DeviceInfo::new_local("MacBook", 45731),
            receive_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        };
        let second = PairedPeer {
            device: DeviceInfo::new_local("Linux Desk", 45731),
            receive_clipboard: false,
            is_default_file_target: true,
            state: PeerConnectionState::Offline,
        };

        let registry = PeerRegistry::from_paired(vec![first.clone(), second.clone()]);

        assert!(registry.discovered().is_empty());
        assert_eq!(registry.paired().len(), 2);
        assert!(registry
            .paired()
            .iter()
            .any(|peer| peer.device.id == first.device.id));
        assert!(registry
            .paired()
            .iter()
            .any(|peer| peer.device.id == second.device.id));
    }

    #[test]
    fn remove_pairing_removes_paired_and_discovered_entries() {
        let device = DeviceInfo::new_local("MacBook", 45731);
        let id = device.id.clone();
        let mut registry = PeerRegistry::from_paired(vec![PairedPeer {
            device: device.clone(),
            receive_clipboard: true,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        }]);
        registry.discovered.insert(id.clone(), device);

        registry.remove_pairing(&id);

        assert!(registry.paired().is_empty());
        assert!(registry.discovered().is_empty());
    }

    #[test]
    fn sync_preferences_preserves_runtime_state() {
        let mut peer = PairedPeer {
            device: DeviceInfo::new_local("MacBook", 45731),
            receive_clipboard: false,
            is_default_file_target: false,
            state: PeerConnectionState::Connected,
        };
        let id = peer.device.id.clone();
        let mut registry = PeerRegistry::from_paired(vec![peer.clone()]);
        peer.receive_clipboard = true;
        peer.is_default_file_target = true;
        peer.state = PeerConnectionState::Offline;

        registry.sync_preferences(&[peer]);

        let paired = registry.paired();
        assert_eq!(paired.len(), 1);
        assert_eq!(paired[0].device.id, id);
        assert!(paired[0].receive_clipboard);
        assert!(paired[0].is_default_file_target);
        assert_eq!(paired[0].state, PeerConnectionState::Connected);
    }
}
