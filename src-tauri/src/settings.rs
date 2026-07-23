use crate::domain::{default_ui_locale, DeviceInfo, LocalSettings, PairedPeer};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const DEFAULT_DISCOVERY_PORT: u16 = 45731;

#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load_or_create(&self, device_name: &str) -> Result<LocalSettings> {
        if self.path.exists() {
            let raw = fs::read_to_string(&self.path)
                .with_context(|| format!("failed to read settings file {}", self.path.display()))?;
            let settings = serde_json::from_str(&raw).with_context(|| {
                format!("failed to parse settings file {}", self.path.display())
            })?;
            return Ok(settings);
        }

        let settings = LocalSettings {
            local_device: DeviceInfo::new_local(device_name, DEFAULT_DISCOVERY_PORT),
            paired_peers: Vec::new(),
            ui_locale: default_ui_locale(),
        };
        self.save(&settings)?;
        Ok(settings)
    }

    pub fn save(&self, settings: &LocalSettings) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create settings directory {}", parent.display())
            })?;
        }

        let raw = serde_json::to_string_pretty(settings).context("failed to serialize settings")?;
        fs::write(&self.path, raw)
            .with_context(|| format!("failed to write settings file {}", self.path.display()))?;
        Ok(())
    }

    pub fn add_or_update_peer(&self, peer: PairedPeer) -> Result<LocalSettings> {
        let mut settings = self.load_or_create("LAN Cross Sync")?;
        settings
            .paired_peers
            .retain(|p| p.device.id != peer.device.id);
        settings.paired_peers.push(peer);
        self.save(&settings)?;
        Ok(settings)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PairedPeer, PeerConnectionState};

    #[test]
    fn load_or_create_persists_default_settings() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));

        let settings = store.load_or_create("Windows Desk").unwrap();
        let loaded = store.load_or_create("Ignored Name").unwrap();

        assert_eq!(settings, loaded);
        assert_eq!(loaded.local_device.name, "Windows Desk");
        assert!(store.path().exists());
    }

    #[test]
    fn load_or_create_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("nested/config/settings.json"));

        store.load_or_create("Windows Desk").unwrap();
        let raw = fs::read_to_string(store.path()).unwrap();
        let settings: LocalSettings = serde_json::from_str(&raw).unwrap();

        assert!(store.path().exists());
        assert_eq!(settings.local_device.name, "Windows Desk");
    }

    #[test]
    fn add_or_update_peer_replaces_existing_device() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let device = DeviceInfo::new_local("MacBook", DEFAULT_DISCOVERY_PORT);

        let first = PairedPeer {
            device: device.clone(),
            receive_clipboard: false,
            is_default_file_target: false,
            state: PeerConnectionState::Offline,
        };
        let second = PairedPeer {
            device,
            receive_clipboard: true,
            is_default_file_target: true,
            state: PeerConnectionState::Connected,
        };

        store.add_or_update_peer(first).unwrap();
        let settings = store.add_or_update_peer(second).unwrap();
        let reloaded = store.load_or_create("Ignored").unwrap();

        assert_eq!(settings.paired_peers.len(), 1);
        assert!(settings.paired_peers[0].receive_clipboard);
        assert!(settings.paired_peers[0].is_default_file_target);
        assert_eq!(
            settings.paired_peers[0].state,
            PeerConnectionState::Connected
        );
        assert_eq!(reloaded.paired_peers.len(), 1);
        assert!(reloaded.paired_peers[0].receive_clipboard);
        assert!(reloaded.paired_peers[0].is_default_file_target);
        assert_eq!(
            reloaded.paired_peers[0].state,
            PeerConnectionState::Connected
        );
    }

    #[test]
    fn settings_include_simplified_chinese_as_default_locale() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));

        let settings = store.load_or_create("Windows Desk").unwrap();
        let json = serde_json::to_value(settings).unwrap();

        assert_eq!(json["ui_locale"], "zh-CN");
    }
}
