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

    pub fn mark_discovered(&mut self, device: DeviceInfo) {
        if !self.paired.contains_key(&device.id) {
            self.discovered.insert(device.id.clone(), device);
        } else if let Some(peer) = self.paired.get_mut(&device.id) {
            peer.device = device;
            peer.state = PeerConnectionState::Discovered;
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
}
