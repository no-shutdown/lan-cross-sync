use crate::{
    domain::{DeviceId, DeviceInfo},
    transport::{TransportMessage, TransportRuntime},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio::{io::AsyncReadExt, sync::mpsc};
use uuid::Uuid;

pub const FILE_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_MANIFEST_ENTRIES: usize = 10_000;
pub const STAGING_DIRECTORY_NAME: &str = ".lan-cross-sync-staging";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileOffer {
    pub transfer_id: String,
    pub manifest: TransferManifest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileAccept {
    pub transfer_id: String,
    pub accepted: bool,
    pub reason_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileChunk {
    pub transfer_id: String,
    pub relative_path: String,
    pub offset: u64,
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileComplete {
    pub transfer_id: String,
    pub success: bool,
    pub reason_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileCancel {
    pub transfer_id: String,
    pub reason_code: Option<String>,
}

#[derive(Debug, Error)]
pub enum FileTransferError {
    #[error("file operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("unsafe relative path: {0}")]
    UnsafePath(String),
    #[error("unsupported file system entry: {0}")]
    UnsupportedEntry(String),
    #[error("transfer selection is empty")]
    EmptySelection,
    #[error("transfer manifest is too large")]
    ManifestTooLarge,
    #[error("duplicate transfer path: {0}")]
    DuplicatePath(String),
    #[error("invalid transfer state: {0}")]
    InvalidState(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ManifestEntryKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub relative_path: String,
    pub kind: ManifestEntryKind,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TransferManifest {
    pub root_name: String,
    pub total_bytes: u64,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub absolute_path: PathBuf,
    pub relative_path: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct TransferPlan {
    pub manifest: TransferManifest,
    pub source_files: Vec<SourceFile>,
}

pub fn build_transfer_plan(paths: &[PathBuf]) -> Result<TransferPlan, FileTransferError> {
    if paths.is_empty() {
        return Err(FileTransferError::EmptySelection);
    }

    let mut entries = Vec::new();
    let mut source_files = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut root_name = None;

    for path in paths {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(FileTransferError::UnsupportedEntry(
                path.display().to_string(),
            ));
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| FileTransferError::UnsafePath(path.display().to_string()))?;
        let name = name.to_string();
        if root_name.is_none() {
            root_name = Some(name.clone());
        }

        if metadata.is_file() {
            push_entry(
                &mut entries,
                &mut source_files,
                &mut seen_paths,
                path,
                name,
                ManifestEntryKind::File,
                metadata.len(),
            )?;
        } else if metadata.is_dir() {
            collect_directory(
                path,
                &name,
                &mut entries,
                &mut source_files,
                &mut seen_paths,
            )?;
        } else {
            return Err(FileTransferError::UnsupportedEntry(
                path.display().to_string(),
            ));
        }
    }

    if entries.len() > MAX_MANIFEST_ENTRIES {
        return Err(FileTransferError::ManifestTooLarge);
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    source_files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let total_bytes = entries
        .iter()
        .filter(|entry| entry.kind == ManifestEntryKind::File)
        .map(|entry| entry.size)
        .sum();

    Ok(TransferPlan {
        manifest: TransferManifest {
            root_name: root_name.unwrap_or_default(),
            total_bytes,
            entries,
        },
        source_files,
    })
}

fn collect_directory(
    root: &Path,
    root_name: &str,
    entries: &mut Vec<ManifestEntry>,
    source_files: &mut Vec<SourceFile>,
    seen_paths: &mut HashSet<String>,
) -> Result<(), FileTransferError> {
    push_entry(
        entries,
        source_files,
        seen_paths,
        root,
        root_name.to_string(),
        ManifestEntryKind::Directory,
        0,
    )?;

    let mut children = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, io::Error>>()?;
    children.sort();
    for child in children {
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.file_type().is_symlink() {
            return Err(FileTransferError::UnsupportedEntry(
                child.display().to_string(),
            ));
        }
        let relative_suffix = child
            .strip_prefix(root)
            .map_err(|_| FileTransferError::UnsafePath(child.display().to_string()))?;
        let relative_suffix = relative_suffix_to_wire(relative_suffix)?;
        let relative_path = format!("{root_name}/{relative_suffix}");
        if metadata.is_dir() {
            collect_directory_at_relative(
                &child,
                &relative_path,
                entries,
                source_files,
                seen_paths,
            )?;
        } else if metadata.is_file() {
            push_entry(
                entries,
                source_files,
                seen_paths,
                &child,
                relative_path,
                ManifestEntryKind::File,
                metadata.len(),
            )?;
        } else {
            return Err(FileTransferError::UnsupportedEntry(
                child.display().to_string(),
            ));
        }
    }
    Ok(())
}

fn collect_directory_at_relative(
    directory: &Path,
    relative_path: &str,
    entries: &mut Vec<ManifestEntry>,
    source_files: &mut Vec<SourceFile>,
    seen_paths: &mut HashSet<String>,
) -> Result<(), FileTransferError> {
    push_entry(
        entries,
        source_files,
        seen_paths,
        directory,
        relative_path.to_string(),
        ManifestEntryKind::Directory,
        0,
    )?;

    let mut children = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, io::Error>>()?;
    children.sort();
    for child in children {
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.file_type().is_symlink() {
            return Err(FileTransferError::UnsupportedEntry(
                child.display().to_string(),
            ));
        }
        let suffix = child
            .strip_prefix(directory)
            .map_err(|_| FileTransferError::UnsafePath(child.display().to_string()))?;
        let suffix = relative_suffix_to_wire(suffix)?;
        let child_relative_path = format!("{relative_path}/{suffix}");
        if metadata.is_dir() {
            collect_directory_at_relative(
                &child,
                &child_relative_path,
                entries,
                source_files,
                seen_paths,
            )?;
        } else if metadata.is_file() {
            push_entry(
                entries,
                source_files,
                seen_paths,
                &child,
                child_relative_path,
                ManifestEntryKind::File,
                metadata.len(),
            )?;
        } else {
            return Err(FileTransferError::UnsupportedEntry(
                child.display().to_string(),
            ));
        }
    }
    Ok(())
}

fn push_entry(
    entries: &mut Vec<ManifestEntry>,
    source_files: &mut Vec<SourceFile>,
    seen_paths: &mut HashSet<String>,
    absolute_path: &Path,
    relative_path: String,
    kind: ManifestEntryKind,
    size: u64,
) -> Result<(), FileTransferError> {
    if !seen_paths.insert(relative_path.clone()) {
        return Err(FileTransferError::DuplicatePath(relative_path));
    }
    entries.push(ManifestEntry {
        relative_path: relative_path.clone(),
        kind: kind.clone(),
        size,
    });
    if kind == ManifestEntryKind::File {
        source_files.push(SourceFile {
            absolute_path: absolute_path.to_path_buf(),
            relative_path,
            size,
        });
    }
    Ok(())
}

fn relative_suffix_to_wire(path: &Path) -> Result<String, FileTransferError> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(FileTransferError::UnsafePath(path.display().to_string()));
        };
        let value = value
            .to_str()
            .filter(|value| !value.is_empty() && !value.contains(['/', '\\', ':']))
            .ok_or_else(|| FileTransferError::UnsafePath(path.display().to_string()))?;
        parts.push(value.to_string());
    }
    if parts.is_empty() {
        return Err(FileTransferError::UnsafePath(path.display().to_string()));
    }
    Ok(parts.join("/"))
}

pub fn safe_destination_path(root: &Path, relative: &str) -> Result<PathBuf, FileTransferError> {
    if relative.is_empty() || relative.contains('\0') || relative.contains(':') {
        return Err(FileTransferError::UnsafePath(relative.to_string()));
    }
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(FileTransferError::UnsafePath(relative.to_string()));
    }
    Ok(root.join(relative_path))
}

pub fn part_destination_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("transfer");
    destination.with_file_name(format!("{name}.part"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferState {
    Preparing,
    Offered,
    Transferring,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferStateMachine {
    state: TransferState,
}

impl TransferStateMachine {
    pub fn new() -> Self {
        Self {
            state: TransferState::Preparing,
        }
    }

    #[cfg(test)]
    pub fn state(&self) -> TransferState {
        self.state
    }

    pub fn offer(&mut self) -> Result<(), FileTransferError> {
        self.transition(TransferState::Preparing, TransferState::Offered)
    }

    pub fn start(&mut self) -> Result<(), FileTransferError> {
        self.transition(TransferState::Offered, TransferState::Transferring)
    }

    pub fn complete(&mut self) -> Result<(), FileTransferError> {
        self.transition(TransferState::Transferring, TransferState::Completed)
    }

    pub fn cancel(&mut self) -> Result<(), FileTransferError> {
        match self.state {
            TransferState::Completed | TransferState::Cancelled => Err(
                FileTransferError::InvalidState(format!("cannot cancel from {:?}", self.state)),
            ),
            _ => {
                self.state = TransferState::Cancelled;
                Ok(())
            }
        }
    }

    fn fail(&mut self) {
        if !matches!(
            self.state,
            TransferState::Completed | TransferState::Cancelled
        ) {
            self.state = TransferState::Failed;
        }
    }

    fn transition(
        &mut self,
        expected: TransferState,
        next: TransferState,
    ) -> Result<(), FileTransferError> {
        if self.state != expected {
            return Err(FileTransferError::InvalidState(format!(
                "expected {:?}, got {:?}",
                expected, self.state
            )));
        }
        self.state = next;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Sending,
    Receiving,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransferEvent {
    Offer {
        transfer_id: String,
        peer: DeviceInfo,
        manifest: TransferManifest,
        direction: TransferDirection,
    },
    Progress {
        transfer_id: String,
        peer_id: DeviceId,
        direction: TransferDirection,
        transferred_bytes: u64,
        total_bytes: u64,
    },
    Completed {
        transfer_id: String,
        peer_id: DeviceId,
        direction: TransferDirection,
    },
    Failed {
        transfer_id: String,
        peer_id: DeviceId,
        direction: TransferDirection,
        reason_code: String,
    },
    Cancelled {
        transfer_id: String,
        peer_id: DeviceId,
        direction: TransferDirection,
    },
}

struct OutgoingTransfer {
    peer_id: DeviceId,
    plan: TransferPlan,
    state: TransferStateMachine,
}

struct IncomingTransfer {
    transfer_id: String,
    peer_id: DeviceId,
    manifest: TransferManifest,
    destination: Option<PathBuf>,
    staging_dir: Option<PathBuf>,
    received_files: HashMap<String, u64>,
    received_bytes: u64,
    state: TransferStateMachine,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StagingRecord {
    transfer_id: String,
    staging_dir: PathBuf,
}

#[derive(Clone)]
pub struct FileTransferService {
    transport: Arc<TransportRuntime>,
    outgoing: Arc<Mutex<HashMap<String, OutgoingTransfer>>>,
    incoming: Arc<Mutex<HashMap<String, IncomingTransfer>>>,
    cancelled: Arc<Mutex<HashSet<String>>>,
    staging_records: Arc<Mutex<HashMap<String, PathBuf>>>,
    staging_index_path: PathBuf,
    events: mpsc::UnboundedSender<TransferEvent>,
}

impl FileTransferService {
    pub fn new(
        transport: Arc<TransportRuntime>,
        staging_index_path: PathBuf,
    ) -> anyhow::Result<(Self, mpsc::UnboundedReceiver<TransferEvent>)> {
        initialize_staging_index(&staging_index_path)?;
        let (events, receiver) = mpsc::unbounded_channel();
        Ok((
            Self {
                transport,
                outgoing: Arc::new(Mutex::new(HashMap::new())),
                incoming: Arc::new(Mutex::new(HashMap::new())),
                cancelled: Arc::new(Mutex::new(HashSet::new())),
                staging_records: Arc::new(Mutex::new(HashMap::new())),
                staging_index_path,
                events,
            },
            receiver,
        ))
    }

    pub async fn start_transfer(
        &self,
        peer_id: DeviceId,
        paths: Vec<PathBuf>,
    ) -> anyhow::Result<String> {
        if !self.transport.is_connected(&peer_id) {
            anyhow::bail!("peer is not connected")
        }
        let plan = build_transfer_plan(&paths)?;
        let transfer_id = Uuid::new_v4().to_string();
        let mut state = TransferStateMachine::new();
        state.offer()?;
        let manifest = plan.manifest.clone();
        self.outgoing.lock().unwrap().insert(
            transfer_id.clone(),
            OutgoingTransfer {
                peer_id: peer_id.clone(),
                plan,
                state,
            },
        );

        let message = TransportMessage::FileOffer(FileOffer {
            transfer_id: transfer_id.clone(),
            manifest: manifest.clone(),
        });
        if let Err(err) = self.transport.send_message(&peer_id, message).await {
            self.outgoing.lock().unwrap().remove(&transfer_id);
            return Err(err.into());
        }
        self.emit(TransferEvent::Offer {
            transfer_id: transfer_id.clone(),
            peer: self
                .transport
                .registry
                .lock()
                .unwrap()
                .device(&peer_id)
                .unwrap_or_else(|| self.transport.local_device.clone()),
            manifest,
            direction: TransferDirection::Sending,
        });
        Ok(transfer_id)
    }

    pub async fn accept_transfer(
        &self,
        transfer_id: &str,
        destination: PathBuf,
    ) -> anyhow::Result<()> {
        let (peer_id, manifest) = {
            let incoming = self.incoming.lock().unwrap();
            let transfer = incoming
                .get(transfer_id)
                .ok_or_else(|| anyhow::anyhow!("transfer not found"))?;
            if transfer.destination.is_some() {
                anyhow::bail!("transfer has already been accepted")
            }
            (transfer.peer_id.clone(), transfer.manifest.clone())
        };

        let staging_dir = staging_directory_for(&destination);
        self.register_staging(transfer_id, staging_dir.clone())?;
        if let Err(err) = prepare_staging_destination(&destination, &staging_dir, &manifest) {
            let _ = self.cleanup_staging_path(transfer_id, &staging_dir);
            return Err(err);
        }
        let staging_for_cleanup = staging_dir.clone();
        let setup_result = (|| -> anyhow::Result<()> {
            let mut incoming = self.incoming.lock().unwrap();
            let transfer = incoming
                .get_mut(transfer_id)
                .ok_or_else(|| anyhow::anyhow!("transfer not found"))?;
            transfer.destination = Some(destination);
            transfer.staging_dir = Some(staging_dir);
            transfer.received_files = manifest
                .entries
                .iter()
                .filter(|entry| entry.kind == ManifestEntryKind::File && entry.size == 0)
                .map(|entry| (entry.relative_path.clone(), 0))
                .collect();
            transfer.state.start()?;
            Ok(())
        })();
        if let Err(err) = setup_result {
            let _ = self.cleanup_staging_path(transfer_id, &staging_for_cleanup);
            return Err(err);
        }
        if let Err(err) = self
            .transport
            .send_message(
                &peer_id,
                TransportMessage::FileAccept(FileAccept {
                    transfer_id: transfer_id.to_string(),
                    accepted: true,
                    reason_code: None,
                }),
            )
            .await
        {
            if let Some(transfer) = self.incoming.lock().unwrap().remove(transfer_id) {
                self.cleanup_incoming(&transfer)?;
            } else {
                self.unregister_staging(transfer_id)?;
            }
            return Err(err.into());
        }
        self.emit(TransferEvent::Progress {
            transfer_id: transfer_id.to_string(),
            peer_id,
            direction: TransferDirection::Receiving,
            transferred_bytes: 0,
            total_bytes: manifest.total_bytes,
        });
        Ok(())
    }

    pub async fn cancel_transfer(&self, transfer_id: &str) -> anyhow::Result<()> {
        let peer_id = {
            let outgoing = self.outgoing.lock().unwrap();
            outgoing
                .get(transfer_id)
                .map(|transfer| (transfer.peer_id.clone(), TransferDirection::Sending))
                .or_else(|| {
                    self.incoming
                        .lock()
                        .unwrap()
                        .get(transfer_id)
                        .map(|transfer| (transfer.peer_id.clone(), TransferDirection::Receiving))
                })
                .ok_or_else(|| anyhow::anyhow!("transfer not found"))?
        };
        self.cancelled
            .lock()
            .unwrap()
            .insert(transfer_id.to_string());
        if peer_id.1 == TransferDirection::Receiving {
            if let Some(mut transfer) = self.incoming.lock().unwrap().remove(transfer_id) {
                let _ = transfer.state.cancel();
                self.cleanup_incoming(&transfer)?;
            }
        }
        self.transport
            .send_message(
                &peer_id.0,
                TransportMessage::FileCancel(FileCancel {
                    transfer_id: transfer_id.to_string(),
                    reason_code: Some("cancelled_by_user".to_string()),
                }),
            )
            .await?;
        self.emit(TransferEvent::Cancelled {
            transfer_id: transfer_id.to_string(),
            peer_id: peer_id.0,
            direction: peer_id.1,
        });
        Ok(())
    }

    pub async fn handle_message(
        &self,
        peer: &DeviceInfo,
        message: TransportMessage,
    ) -> anyhow::Result<()> {
        match message {
            TransportMessage::FileOffer(offer) => self.handle_offer(peer, offer),
            TransportMessage::FileAccept(accept) => self.handle_accept(peer, accept).await,
            TransportMessage::FileChunk(chunk) => self.handle_chunk(peer, chunk),
            TransportMessage::FileComplete(complete) => self.handle_complete(peer, complete),
            TransportMessage::FileCancel(cancel) => self.handle_cancel(peer, cancel),
            _ => Ok(()),
        }
    }

    fn handle_offer(&self, peer: &DeviceInfo, offer: FileOffer) -> anyhow::Result<()> {
        validate_manifest(&offer.manifest)?;
        let mut state = TransferStateMachine::new();
        state.offer()?;
        let transfer_id = offer.transfer_id.clone();
        let manifest = offer.manifest.clone();
        self.incoming.lock().unwrap().insert(
            transfer_id.clone(),
            IncomingTransfer {
                transfer_id: transfer_id.clone(),
                peer_id: peer.id.clone(),
                manifest: manifest.clone(),
                destination: None,
                staging_dir: None,
                received_files: HashMap::new(),
                received_bytes: 0,
                state,
            },
        );
        self.emit(TransferEvent::Offer {
            transfer_id,
            peer: peer.clone(),
            manifest,
            direction: TransferDirection::Receiving,
        });
        Ok(())
    }

    async fn handle_accept(&self, peer: &DeviceInfo, accept: FileAccept) -> anyhow::Result<()> {
        let plan = {
            let mut outgoing = self.outgoing.lock().unwrap();
            let transfer = outgoing
                .get_mut(&accept.transfer_id)
                .ok_or_else(|| anyhow::anyhow!("transfer not found"))?;
            if transfer.peer_id != peer.id {
                anyhow::bail!("transfer peer mismatch")
            }
            if !accept.accepted {
                let transfer = outgoing.remove(&accept.transfer_id).unwrap();
                self.emit(TransferEvent::Failed {
                    transfer_id: accept.transfer_id,
                    peer_id: transfer.peer_id,
                    direction: TransferDirection::Sending,
                    reason_code: accept
                        .reason_code
                        .unwrap_or_else(|| "receiver_rejected".to_string()),
                });
                return Ok(());
            }
            transfer.state.start()?;
            transfer.plan.clone()
        };

        let transfer_id = accept.transfer_id;
        let result = self.stream_outgoing(&peer.id, &transfer_id, &plan).await;
        let success = result.is_ok();
        let reason_code = result.as_ref().err().map(|err| {
            if self.is_cancelled(&transfer_id) {
                "cancelled_by_user".to_string()
            } else {
                format_error_code(err)
            }
        });
        let cancelled = self.is_cancelled(&transfer_id);
        let _ = self
            .transport
            .send_message(
                &peer.id,
                TransportMessage::FileComplete(FileComplete {
                    transfer_id: transfer_id.clone(),
                    success,
                    reason_code: reason_code.clone(),
                }),
            )
            .await;
        if let Some(mut transfer) = self.outgoing.lock().unwrap().remove(&transfer_id) {
            if success {
                transfer.state.complete()?;
            } else if cancelled {
                let _ = transfer.state.cancel();
            } else {
                transfer.state.fail();
            }
        }
        self.cancelled.lock().unwrap().remove(&transfer_id);
        if success {
            self.emit(TransferEvent::Completed {
                transfer_id,
                peer_id: peer.id.clone(),
                direction: TransferDirection::Sending,
            });
        } else if reason_code.as_deref() == Some("cancelled_by_user") {
            self.emit(TransferEvent::Cancelled {
                transfer_id,
                peer_id: peer.id.clone(),
                direction: TransferDirection::Sending,
            });
        } else {
            self.emit(TransferEvent::Failed {
                transfer_id,
                peer_id: peer.id.clone(),
                direction: TransferDirection::Sending,
                reason_code: reason_code.unwrap_or_else(|| "transfer_failed".to_string()),
            });
        }
        Ok(())
    }

    async fn stream_outgoing(
        &self,
        peer_id: &DeviceId,
        transfer_id: &str,
        plan: &TransferPlan,
    ) -> anyhow::Result<()> {
        let mut transferred_bytes = 0_u64;
        for source in &plan.source_files {
            let mut file = tokio::fs::File::open(&source.absolute_path).await?;
            let mut offset = 0_u64;
            loop {
                if self.is_cancelled(transfer_id) {
                    anyhow::bail!("transfer cancelled")
                }
                let mut data = vec![0_u8; FILE_CHUNK_BYTES];
                let read = file.read(&mut data).await?;
                if read == 0 {
                    break;
                }
                data.truncate(read);
                self.transport
                    .send_message(
                        peer_id,
                        TransportMessage::FileChunk(FileChunk {
                            transfer_id: transfer_id.to_string(),
                            relative_path: source.relative_path.clone(),
                            offset,
                            data,
                        }),
                    )
                    .await?;
                offset += read as u64;
                transferred_bytes += read as u64;
                self.emit(TransferEvent::Progress {
                    transfer_id: transfer_id.to_string(),
                    peer_id: peer_id.clone(),
                    direction: TransferDirection::Sending,
                    transferred_bytes,
                    total_bytes: plan.manifest.total_bytes,
                });
            }
            if offset != source.size {
                anyhow::bail!("source file changed during transfer")
            }
        }
        Ok(())
    }

    fn handle_chunk(&self, peer: &DeviceInfo, chunk: FileChunk) -> anyhow::Result<()> {
        let (staging_dir, expected_offset, expected_size, manifest) = {
            let incoming = self.incoming.lock().unwrap();
            let transfer = incoming
                .get(&chunk.transfer_id)
                .ok_or_else(|| anyhow::anyhow!("transfer not found"))?;
            if transfer.peer_id != peer.id {
                anyhow::bail!("transfer peer mismatch")
            }
            let staging_dir = transfer
                .staging_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("receiver has not accepted transfer"))?;
            let entry = transfer
                .manifest
                .entries
                .iter()
                .find(|entry| entry.relative_path == chunk.relative_path)
                .ok_or_else(|| anyhow::anyhow!("file is not in manifest"))?;
            if entry.kind != ManifestEntryKind::File {
                anyhow::bail!("manifest entry is not a file")
            }
            (
                staging_dir,
                transfer
                    .received_files
                    .get(&chunk.relative_path)
                    .copied()
                    .unwrap_or(0),
                entry.size,
                transfer.manifest.clone(),
            )
        };
        if chunk.offset != expected_offset
            || chunk.data.len() > FILE_CHUNK_BYTES
            || expected_offset + chunk.data.len() as u64 > expected_size
        {
            anyhow::bail!("invalid file chunk")
        }

        let staging_path = safe_destination_path(&staging_dir, &chunk.relative_path)?;
        let part_path = part_destination_path(&staging_path);
        if let Some(parent) = part_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(&part_path)?;
        file.seek(SeekFrom::Start(chunk.offset))?;
        file.write_all(&chunk.data)?;
        file.flush()?;
        let received = expected_offset + chunk.data.len() as u64;
        if received == expected_size {
            if staging_path.exists() {
                anyhow::bail!("staged file already exists")
            }
            fs::rename(&part_path, &staging_path)?;
        }

        let mut incoming = self.incoming.lock().unwrap();
        let transfer = incoming
            .get_mut(&chunk.transfer_id)
            .ok_or_else(|| anyhow::anyhow!("transfer not found"))?;
        transfer
            .received_files
            .insert(chunk.relative_path, received);
        transfer.received_bytes += chunk.data.len() as u64;
        let progress = transfer.received_bytes;
        drop(incoming);
        self.emit(TransferEvent::Progress {
            transfer_id: chunk.transfer_id,
            peer_id: peer.id.clone(),
            direction: TransferDirection::Receiving,
            transferred_bytes: progress,
            total_bytes: manifest.total_bytes,
        });
        Ok(())
    }

    fn handle_complete(&self, peer: &DeviceInfo, complete: FileComplete) -> anyhow::Result<()> {
        let mut transfer = {
            let mut incoming = self.incoming.lock().unwrap();
            let peer_matches = incoming
                .get(&complete.transfer_id)
                .ok_or_else(|| anyhow::anyhow!("transfer not found"))?
                .peer_id
                == peer.id;
            if !peer_matches {
                anyhow::bail!("transfer peer mismatch")
            }
            incoming
                .remove(&complete.transfer_id)
                .ok_or_else(|| anyhow::anyhow!("transfer not found"))?
        };
        if !complete.success {
            self.cleanup_incoming(&transfer)?;
            self.emit(TransferEvent::Failed {
                transfer_id: complete.transfer_id,
                peer_id: peer.id.clone(),
                direction: TransferDirection::Receiving,
                reason_code: complete
                    .reason_code
                    .unwrap_or_else(|| "sender_failed".to_string()),
            });
            return Ok(());
        }
        let manifest_complete = transfer
            .manifest
            .entries
            .iter()
            .filter(|entry| entry.kind == ManifestEntryKind::File)
            .all(|entry| {
                transfer
                    .received_files
                    .get(&entry.relative_path)
                    .is_some_and(|received| *received == entry.size)
            });
        if !manifest_complete {
            self.cleanup_incoming(&transfer)?;
            self.emit(TransferEvent::Failed {
                transfer_id: transfer.transfer_id,
                peer_id: peer.id.clone(),
                direction: TransferDirection::Receiving,
                reason_code: "manifest_mismatch".to_string(),
            });
            return Ok(());
        }

        let Some(destination) = transfer.destination.clone() else {
            self.cleanup_incoming(&transfer)?;
            anyhow::bail!("receiver destination is missing")
        };
        let Some(staging_dir) = transfer.staging_dir.clone() else {
            self.cleanup_incoming(&transfer)?;
            anyhow::bail!("receiver staging directory is missing")
        };
        if let Err(err) =
            finalize_staging_destination(&destination, &staging_dir, &transfer.manifest)
        {
            let reason_code = format_error_code(&err);
            let _ = self.cleanup_incoming(&transfer);
            self.emit(TransferEvent::Failed {
                transfer_id: transfer.transfer_id,
                peer_id: peer.id.clone(),
                direction: TransferDirection::Receiving,
                reason_code,
            });
            return Ok(());
        }
        transfer.state.complete()?;
        self.cleanup_incoming(&transfer)?;
        self.emit(TransferEvent::Completed {
            transfer_id: transfer.transfer_id,
            peer_id: peer.id.clone(),
            direction: TransferDirection::Receiving,
        });
        Ok(())
    }

    fn handle_cancel(&self, peer: &DeviceInfo, cancel: FileCancel) -> anyhow::Result<()> {
        let incoming_transfer = {
            let mut incoming = self.incoming.lock().unwrap();
            match incoming.get(&cancel.transfer_id) {
                Some(transfer) if transfer.peer_id != peer.id => {
                    anyhow::bail!("transfer peer mismatch")
                }
                Some(_) => incoming.remove(&cancel.transfer_id),
                None => None,
            }
        };
        if let Some(mut transfer) = incoming_transfer {
            let _ = transfer.state.cancel();
            self.cleanup_incoming(&transfer)?;
            self.emit(TransferEvent::Cancelled {
                transfer_id: cancel.transfer_id,
                peer_id: peer.id.clone(),
                direction: TransferDirection::Receiving,
            });
            return Ok(());
        }

        let outgoing_offer = {
            let mut outgoing = self.outgoing.lock().unwrap();
            match outgoing.get(&cancel.transfer_id) {
                Some(transfer) if transfer.peer_id != peer.id => {
                    anyhow::bail!("transfer peer mismatch")
                }
                Some(transfer) if transfer.state.state == TransferState::Offered => {
                    outgoing.remove(&cancel.transfer_id)
                }
                Some(_) => None,
                None => None,
            }
        };
        self.cancelled
            .lock()
            .unwrap()
            .insert(cancel.transfer_id.clone());
        if let Some(transfer) = outgoing_offer {
            self.cancelled.lock().unwrap().remove(&cancel.transfer_id);
            self.emit(TransferEvent::Cancelled {
                transfer_id: cancel.transfer_id,
                peer_id: transfer.peer_id,
                direction: TransferDirection::Sending,
            });
        }
        Ok(())
    }

    pub fn handle_peer_disconnected(&self, peer_id: &DeviceId) {
        let incoming = {
            let mut incoming = self.incoming.lock().unwrap();
            let transfer_ids = incoming
                .iter()
                .filter(|(_, transfer)| transfer.peer_id == *peer_id)
                .map(|(transfer_id, _)| transfer_id.clone())
                .collect::<Vec<_>>();
            transfer_ids
                .into_iter()
                .filter_map(|transfer_id| incoming.remove(&transfer_id))
                .collect::<Vec<_>>()
        };
        for transfer in incoming {
            if let Err(err) = self.cleanup_incoming(&transfer) {
                tracing::warn!(?err, transfer_id = %transfer.transfer_id, "failed to clean interrupted incoming transfer");
            }
            self.emit(TransferEvent::Failed {
                transfer_id: transfer.transfer_id,
                peer_id: peer_id.clone(),
                direction: TransferDirection::Receiving,
                reason_code: "peer_disconnected".to_string(),
            });
        }

        let (pending_outgoing, active_outgoing) = {
            let outgoing = self.outgoing.lock().unwrap();
            outgoing
                .iter()
                .filter(|(_, transfer)| transfer.peer_id == *peer_id)
                .fold(
                    (Vec::new(), Vec::new()),
                    |mut ids, (transfer_id, transfer)| {
                        if transfer.state.state == TransferState::Offered {
                            ids.0.push(transfer_id.clone());
                        } else {
                            ids.1.push(transfer_id.clone());
                        }
                        ids
                    },
                )
        };
        for transfer_id in &active_outgoing {
            self.cancelled.lock().unwrap().insert(transfer_id.clone());
        }
        if !pending_outgoing.is_empty() {
            let mut outgoing = self.outgoing.lock().unwrap();
            for transfer_id in pending_outgoing {
                if let Some(transfer) = outgoing.remove(&transfer_id) {
                    self.emit(TransferEvent::Failed {
                        transfer_id,
                        peer_id: transfer.peer_id,
                        direction: TransferDirection::Sending,
                        reason_code: "peer_disconnected".to_string(),
                    });
                }
            }
        }
    }

    fn register_staging(&self, transfer_id: &str, staging_dir: PathBuf) -> anyhow::Result<()> {
        let mut records = self.staging_records.lock().unwrap();
        let previous = records.insert(transfer_id.to_string(), staging_dir);
        if let Err(err) = persist_staging_records(&self.staging_index_path, &records) {
            if let Some(previous) = previous {
                records.insert(transfer_id.to_string(), previous);
            } else {
                records.remove(transfer_id);
            }
            return Err(err);
        }
        Ok(())
    }

    fn unregister_staging(&self, transfer_id: &str) -> anyhow::Result<()> {
        let mut records = self.staging_records.lock().unwrap();
        let Some(previous) = records.remove(transfer_id) else {
            return Ok(());
        };
        if let Err(err) = persist_staging_records(&self.staging_index_path, &records) {
            records.insert(transfer_id.to_string(), previous);
            return Err(err);
        }
        Ok(())
    }

    fn cleanup_staging_path(&self, transfer_id: &str, staging_dir: &Path) -> anyhow::Result<()> {
        cleanup_staging_directory(staging_dir)?;
        self.unregister_staging(transfer_id)
    }

    fn cleanup_incoming(&self, transfer: &IncomingTransfer) -> anyhow::Result<()> {
        if let Some(staging_dir) = &transfer.staging_dir {
            self.cleanup_staging_path(&transfer.transfer_id, staging_dir)?;
        } else {
            self.unregister_staging(&transfer.transfer_id)?;
        }
        Ok(())
    }

    fn emit(&self, event: TransferEvent) {
        let _ = self.events.send(event);
    }

    fn is_cancelled(&self, transfer_id: &str) -> bool {
        self.cancelled.lock().unwrap().contains(transfer_id)
    }
}

fn validate_manifest(manifest: &TransferManifest) -> anyhow::Result<()> {
    if manifest.entries.is_empty() || manifest.entries.len() > MAX_MANIFEST_ENTRIES {
        anyhow::bail!("invalid transfer manifest")
    }
    let mut paths = HashSet::new();
    for entry in &manifest.entries {
        safe_destination_path(Path::new("."), &entry.relative_path)?;
        if !paths.insert(entry.relative_path.clone()) {
            anyhow::bail!("duplicate transfer path")
        }
    }
    Ok(())
}

fn staging_directory_for(destination: &Path) -> PathBuf {
    destination
        .join(STAGING_DIRECTORY_NAME)
        .join(Uuid::new_v4().to_string())
}

fn prepare_staging_destination(
    destination: &Path,
    staging_dir: &Path,
    manifest: &TransferManifest,
) -> anyhow::Result<()> {
    validate_manifest(manifest)?;
    for entry in &manifest.entries {
        let path = safe_destination_path(destination, &entry.relative_path)?;
        match entry.kind {
            ManifestEntryKind::Directory => {
                if path.exists() && !path.is_dir() {
                    anyhow::bail!("destination directory already exists as a file")
                }
            }
            ManifestEntryKind::File => {
                if path.exists() {
                    anyhow::bail!("destination file already exists")
                }
            }
        }
    }

    fs::create_dir_all(staging_dir)?;
    for entry in &manifest.entries {
        let path = safe_destination_path(staging_dir, &entry.relative_path)?;
        match entry.kind {
            ManifestEntryKind::Directory => fs::create_dir_all(path)?,
            ManifestEntryKind::File => {
                if entry.size == 0 {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    OpenOptions::new().create_new(true).write(true).open(path)?;
                }
            }
        }
    }
    Ok(())
}

fn finalize_staging_destination(
    destination: &Path,
    staging_dir: &Path,
    manifest: &TransferManifest,
) -> anyhow::Result<()> {
    validate_manifest(manifest)?;
    for entry in &manifest.entries {
        let path = safe_destination_path(destination, &entry.relative_path)?;
        match entry.kind {
            ManifestEntryKind::Directory => {
                if path.exists() && !path.is_dir() {
                    anyhow::bail!("destination directory already exists as a file")
                }
            }
            ManifestEntryKind::File => {
                if path.exists() {
                    anyhow::bail!("destination file already exists")
                }
            }
        }
    }

    for entry in &manifest.entries {
        if entry.kind != ManifestEntryKind::File {
            continue;
        }
        let staged_path = safe_destination_path(staging_dir, &entry.relative_path)?;
        let metadata = fs::metadata(&staged_path)
            .map_err(|_| anyhow::anyhow!("staged file is missing: {}", entry.relative_path))?;
        if metadata.len() != entry.size {
            anyhow::bail!(
                "staged file has an unexpected size: {}",
                entry.relative_path
            )
        }
    }

    fs::create_dir_all(destination)?;
    for entry in &manifest.entries {
        if entry.kind != ManifestEntryKind::Directory {
            continue;
        }
        fs::create_dir_all(safe_destination_path(destination, &entry.relative_path)?)?;
    }

    for entry in &manifest.entries {
        if entry.kind != ManifestEntryKind::File {
            continue;
        }
        let staged_path = safe_destination_path(staging_dir, &entry.relative_path)?;
        let destination_path = safe_destination_path(destination, &entry.relative_path)?;
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(staged_path, destination_path)?;
    }
    Ok(())
}

fn initialize_staging_index(index_path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let records = if index_path.exists() {
        let raw = fs::read_to_string(index_path)?;
        serde_json::from_str::<Vec<StagingRecord>>(&raw)?
    } else {
        Vec::new()
    };
    for record in records {
        if is_managed_staging_directory(&record.staging_dir) {
            cleanup_staging_directory(&record.staging_dir)?;
        }
    }
    persist_staging_records(index_path, &HashMap::new())
}

fn persist_staging_records(
    index_path: &Path,
    records: &HashMap<String, PathBuf>,
) -> anyhow::Result<()> {
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut serialized = records
        .iter()
        .map(|(transfer_id, staging_dir)| StagingRecord {
            transfer_id: transfer_id.clone(),
            staging_dir: staging_dir.clone(),
        })
        .collect::<Vec<_>>();
    serialized.sort_by(|left, right| left.transfer_id.cmp(&right.transfer_id));
    fs::write(index_path, serde_json::to_string_pretty(&serialized)?)?;
    Ok(())
}

fn is_managed_staging_directory(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some(STAGING_DIRECTORY_NAME)
}

fn cleanup_staging_directory(path: &Path) -> anyhow::Result<()> {
    if !is_managed_staging_directory(path) {
        anyhow::bail!("unsafe staging directory: {}", path.display())
    }

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir(parent);
    }
    Ok(())
}

fn format_error_code(error: &anyhow::Error) -> String {
    if error.to_string().contains("source file changed") {
        "source_changed".to_string()
    } else if error.to_string().contains("cancelled") {
        "cancelled_by_user".to_string()
    } else {
        "transfer_failed".to_string()
    }
}

mod base64_bytes {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn transfer_plan_preserves_nested_folder_structure() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Photos");
        fs::create_dir_all(folder.join("2026")).unwrap();
        fs::write(folder.join("cover.txt"), b"cover").unwrap();
        fs::write(folder.join("2026").join("note.txt"), b"note").unwrap();

        let plan = build_transfer_plan(&[folder]).unwrap();

        assert!(plan
            .manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == "Photos/cover.txt"));
        assert!(plan
            .manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == "Photos/2026/note.txt"));
        assert_eq!(plan.manifest.total_bytes, 9);
    }

    #[test]
    fn unsafe_relative_paths_are_rejected() {
        let root = tempfile::tempdir().unwrap();

        assert!(matches!(
            safe_destination_path(root.path(), "../escape.txt"),
            Err(FileTransferError::UnsafePath(_))
        ));
        assert!(matches!(
            safe_destination_path(root.path(), "C:/escape.txt"),
            Err(FileTransferError::UnsafePath(_))
        ));
        assert!(matches!(
            safe_destination_path(root.path(), "/escape.txt"),
            Err(FileTransferError::UnsafePath(_))
        ));
    }

    #[test]
    fn partial_file_uses_part_suffix_until_atomic_finalize() {
        let root = tempfile::tempdir().unwrap();
        let destination = safe_destination_path(root.path(), "Photos/cover.txt").unwrap();
        let part = part_destination_path(&destination);

        assert_eq!(part.file_name().unwrap(), "cover.txt.part");
        assert_ne!(destination, part);
    }

    #[test]
    fn empty_files_are_materialized_during_destination_preparation() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        let staging = destination.join(STAGING_DIRECTORY_NAME).join("transfer-1");
        let manifest = TransferManifest {
            root_name: "Empty".to_string(),
            total_bytes: 0,
            entries: vec![
                ManifestEntry {
                    relative_path: "Empty".to_string(),
                    kind: ManifestEntryKind::Directory,
                    size: 0,
                },
                ManifestEntry {
                    relative_path: "Empty/blank.txt".to_string(),
                    kind: ManifestEntryKind::File,
                    size: 0,
                },
            ],
        };

        prepare_staging_destination(&destination, &staging, &manifest).unwrap();

        assert!(staging.join("Empty/blank.txt").is_file());
        assert!(!destination.join("Empty/blank.txt").exists());
    }

    #[test]
    fn staging_preparation_keeps_destination_clean_until_finalize() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        let staging = destination.join(STAGING_DIRECTORY_NAME).join("transfer-1");
        let manifest = TransferManifest {
            root_name: "Photos".to_string(),
            total_bytes: 5,
            entries: vec![
                ManifestEntry {
                    relative_path: "Photos".to_string(),
                    kind: ManifestEntryKind::Directory,
                    size: 0,
                },
                ManifestEntry {
                    relative_path: "Photos/note.txt".to_string(),
                    kind: ManifestEntryKind::File,
                    size: 5,
                },
            ],
        };

        prepare_staging_destination(&destination, &staging, &manifest).unwrap();
        fs::write(staging.join("Photos/note.txt"), b"hello").unwrap();

        assert!(!destination.join("Photos/note.txt").exists());
        finalize_staging_destination(&destination, &staging, &manifest).unwrap();
        assert_eq!(
            fs::read(destination.join("Photos/note.txt")).unwrap(),
            b"hello"
        );
        cleanup_staging_directory(&staging).unwrap();
        assert!(!staging.exists());
    }

    #[test]
    fn stale_staging_index_is_cleaned_on_startup() {
        let root = tempfile::tempdir().unwrap();
        let staging = root
            .path()
            .join("destination")
            .join(STAGING_DIRECTORY_NAME)
            .join("stale");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("file.part"), b"partial").unwrap();
        let index = root.path().join("cache").join("active-transfers.json");
        let records = vec![StagingRecord {
            transfer_id: "transfer-1".to_string(),
            staging_dir: staging.clone(),
        }];
        fs::create_dir_all(index.parent().unwrap()).unwrap();
        fs::write(&index, serde_json::to_vec(&records).unwrap()).unwrap();

        initialize_staging_index(&index).unwrap();

        assert!(!staging.exists());
        let remaining: Vec<StagingRecord> =
            serde_json::from_str(&fs::read_to_string(index).unwrap()).unwrap();
        assert!(remaining.is_empty());
    }

    #[test]
    fn file_chunk_wire_encoding_round_trips_binary_data() {
        let chunk = FileChunk {
            transfer_id: "transfer-1".to_string(),
            relative_path: "Photos/image.bin".to_string(),
            offset: 12,
            data: vec![0, 1, 2, 255],
        };

        let encoded = serde_json::to_vec(&chunk).unwrap();
        let decoded: FileChunk = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(decoded, chunk);
    }

    #[test]
    fn transfer_state_machine_rejects_invalid_progression_and_supports_cancel() {
        let mut state = TransferStateMachine::new();

        assert!(state.offer().is_ok());
        assert!(state.start().is_ok());
        assert!(state.cancel().is_ok());
        assert_eq!(state.state(), TransferState::Cancelled);
        assert!(matches!(
            state.complete(),
            Err(FileTransferError::InvalidState(_))
        ));
    }
}
