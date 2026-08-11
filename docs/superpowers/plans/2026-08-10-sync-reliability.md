# Clipboard and File Transfer Reliability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Reliably synchronize button-driven clipboard copies and large screenshots, and make file transfers cancel and clean up deterministically on failures or reconnects.

**Architecture:** Keep the existing Rust-owned clipboard, transport, and file-transfer services. Add bounded clipboard-read retries, encode images as PNG, and stream image bytes through small framed messages. Move outgoing file streaming into an owned task, turn receiver chunk failures into explicit cancellation, and guard transport teardown with connection tokens.

**Tech Stack:** Rust 2021, Tokio, Tauri v2, Serde/JSON, arboard, image PNG codec, React/TypeScript build, Cargo tests.

---

## File Map

Modify:

- src-tauri/Cargo.toml and src-tauri/Cargo.lock: add the PNG-only image dependency.
- src-tauri/src/protocol.rs: bump the business protocol version from 2 to 3.
- src-tauri/src/transport.rs: add clipboard image message variants and token-guarded teardown.
- src-tauri/src/clipboard.rs: retry reads, PNG encode/decode, image chunk send/receive, and validation.
- src-tauri/src/file_transfer.rs: asynchronous outgoing streaming, receiver failure cancellation, and finalization rollback.
- src-tauri/src/lib.rs: route image messages and clear clipboard image state on peer disconnect.
- README.md and docs/PROJECT_GUIDE.md: update limits/compatibility only if the final behavior differs from their current text.

No new source files are required.

---

### Task 1: Add PNG support and protocol-v3 image wire types

**Files:**

- Modify: src-tauri/Cargo.toml
- Modify: src-tauri/Cargo.lock
- Modify: src-tauri/src/protocol.rs:4
- Modify: src-tauri/src/clipboard.rs:1-53
- Modify: src-tauri/src/transport.rs:1-112
- Test: src-tauri/src/transport.rs

- [ ] Step 1: Add the dependency.

Add this exact declaration to src-tauri/Cargo.toml:

~~~toml
image = { version = "0.25", default-features = false, features = ["png"] }
~~~

- [ ] Step 2: Write and run the failing wire test.

Add this test to the transport test module:

~~~rust
#[test]
fn clipboard_image_chunk_messages_round_trip_binary_data() {
    let message = TransportMessage::ClipboardImageChunk(ClipboardImageChunk {
        event_id: "event-1".to_string(),
        offset: 65_536,
        data: vec![0, 1, 2, 255],
    });
    let encoded = serde_json::to_vec(&message).unwrap();
    let decoded: TransportMessage = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, message);
}
~~~

Run:

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml transport::tests::clipboard_image_chunk_messages_round_trip_binary_data -- --nocapture
~~~

Expected: FAIL because the type and variant do not exist.

- [ ] Step 3: Implement the wire types and version bump.

Add these serializable types to clipboard.rs:

~~~rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClipboardImageStart {
    pub event_id: String,
    pub source_device_id: DeviceId,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub content_hash: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClipboardImageChunk {
    pub event_id: String,
    pub offset: u64,
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClipboardImageComplete {
    pub event_id: String,
}
~~~

Import the three types in transport.rs and add these variants after Clipboard(ClipboardEvent):

~~~rust
ClipboardImageStart(ClipboardImageStart),
ClipboardImageChunk(ClipboardImageChunk),
ClipboardImageComplete(ClipboardImageComplete),
~~~

Set PROTOCOL_VERSION in protocol.rs to 3 and update its version assertion. Keep the 8 MiB frame limit.

- [ ] Step 4: Run the focused test and commit.

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml transport::tests::clipboard_image_chunk_messages_round_trip_binary_data -- --nocapture
cargo fmt --manifest-path src-tauri/Cargo.toml --all
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/protocol.rs src-tauri/src/clipboard.rs src-tauri/src/transport.rs
git commit -m "feat: add chunked clipboard image wire types"
~~~

Expected: the new test passes and Cargo resolves only the PNG codec dependency tree.

---

### Task 2: Retry transient clipboard reads

**Files:**

- Modify: src-tauri/src/clipboard.rs:8-29,153-233
- Test: src-tauri/src/clipboard.rs

- [ ] Step 1: Write and run failing pure retry tests.

Add tests that never access the host clipboard:

~~~rust
#[test]
fn clipboard_read_retries_transient_provider_failure() {
    let mut attempts = 0;
    let result = retry_clipboard_read(
        || {
            attempts += 1;
            if attempts < 3 {
                Err(ClipboardError::System("clipboard busy".to_string()))
            } else {
                Ok(Some(ClipboardPayload::Text {
                    text: "copied by a button".to_string(),
                }))
            }
        },
        |_| {},
        5,
    )
    .unwrap();
    assert_eq!(attempts, 3);
    assert!(matches!(
        result,
        Some(ClipboardPayload::Text { text }) if text == "copied by a button"
    ));
}

#[test]
fn clipboard_read_returns_last_error_after_retry_budget() {
    let mut attempts = 0;
    let result = retry_clipboard_read(
        || {
            attempts += 1;
            Err(ClipboardError::System(format!("busy-{attempts}")))
        },
        |_| {},
        3,
    );
    assert!(matches!(
        result,
        Err(ClipboardError::System(message)) if message == "busy-3"
    ));
    assert_eq!(attempts, 3);
}
~~~

Run:

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml clipboard::tests::clipboard_read_retries_transient_provider_failure -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml clipboard::tests::clipboard_read_returns_last_error_after_retry_budget -- --nocapture
~~~

Expected: FAIL because retry_clipboard_read does not exist.

- [ ] Step 2: Implement the bounded retry helper.

Use a generic wait callback so tests do not sleep:

~~~rust
fn retry_clipboard_read<F, W>(
    mut read: F,
    mut wait: W,
    attempts: usize,
) -> Result<Option<ClipboardPayload>, ClipboardError>
where
    F: FnMut() -> Result<Option<ClipboardPayload>, ClipboardError>,
    W: FnMut(usize),
{
    let attempts = attempts.max(1);
    let mut last_error = None;
    for attempt in 0..attempts {
        match read() {
            Ok(Some(payload)) => return Ok(Some(payload)),
            Ok(None) => {}
            Err(error) => last_error = Some(error),
        }
        if attempt + 1 < attempts {
            wait(attempt);
        }
    }
    last_error.map_or(Ok(None), Err)
}
~~~

Define production constants for five attempts and a 20 ms wait. Wrap read_system_clipboard with the helper and std::thread::sleep; call the wrapper from process_local_change inside the existing spawn_blocking. Retry both transient errors and Ok(None).

While touching run, remove the cfg-specific needless return so clippy can pass: the Windows branch ends with self.run_windows().await and the non-Windows branch ends with self.run_polling().await.

- [ ] Step 3: Add the empty-format regression and verify.

Add a test where the first read returns Ok(None) and the second returns text. Run:

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml clipboard::tests -- --nocapture
~~~

Expected: all clipboard tests pass, including the retry cases.

- [ ] Step 4: Commit the race fix.

~~~powershell
git add src-tauri/src/clipboard.rs
git commit -m "fix: retry transient clipboard reads"
~~~

---

### Task 3: Encode and stream large clipboard images

**Files:**

- Modify: src-tauri/src/clipboard.rs:29-112,188-263,305-398
- Modify: src-tauri/src/lib.rs:201-249
- Test: src-tauri/src/clipboard.rs

- [ ] Step 1: Write and run failing codec/assembly tests.

Add tests for real PNG conversion and contiguous offsets:

~~~rust
#[test]
fn clipboard_image_round_trips_through_png() {
    let source = vec![
        255, 0, 0, 255, 0, 255, 0, 255,
        0, 0, 255, 255, 255, 255, 0, 255,
    ];
    let encoded = encode_png_image(2, 2, &source).unwrap();
    let decoded = decode_png_image(2, 2, &encoded).unwrap();
    assert_eq!(decoded, source);
}

#[test]
fn clipboard_image_chunk_assembly_requires_contiguous_offsets() {
    let device = DeviceInfo::new_local("MacBook", 45731);
    let png = encode_png_image(2, 2, &[0; 16]).unwrap();
    let event = ClipboardEvent::from_image(device.id.clone(), 1, 10, 2, 2, png).unwrap();
    let start = ClipboardImageStart {
        event_id: event.event_id.clone(),
        source_device_id: event.source_device_id.clone(),
        sequence: event.sequence,
        timestamp_ms: event.timestamp_ms,
        content_hash: event.content_hash.clone(),
        mime_type: "image/png".to_string(),
        width: 2,
        height: 2,
        total_bytes: match &event.payload {
            ClipboardPayload::Image { data, .. } => data.len() as u64,
            ClipboardPayload::Text { .. } => unreachable!(),
        },
    };
    let mut incoming = IncomingImageTransfer::from_start(&device.id, &start).unwrap();
    assert!(incoming.accept_chunk(0, vec![0; 4]).is_ok());
    assert!(incoming.accept_chunk(5, vec![0; 3]).is_err());
}
~~~

Run:

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml clipboard::tests::clipboard_image_round_trips_through_png -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml clipboard::tests::clipboard_image_chunk_assembly_requires_contiguous_offsets -- --nocapture
~~~

Expected: FAIL because the codec helpers and assembly type do not exist.

- [ ] Step 2: Implement PNG conversion and limits.

Add:

~~~rust
pub const CLIPBOARD_IMAGE_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_RAW_IMAGE_BYTES: usize = 128 * 1024 * 1024;
~~~

Implement encode_png_image(width, height, rgba) with image::RgbaImage::from_raw and DynamicImage::write_to(..., ImageFormat::Png). Implement decode_png_image(width, height, encoded) with image::load_from_memory, to_rgba8, exact dimension checks, and raw-byte-limit checks. Preserve ImageTooLarge and InvalidImage error categories.

Change read_system_clipboard to validate raw RGBA data, encode PNG, and set mime_type: "image/png". Change write_system_clipboard to decode PNG before constructing ImageData. ClipboardEvent::from_image must validate PNG bytes and hash the encoded bytes consistently.

- [ ] Step 3: Implement bounded incoming image assembly.

Add IncomingImageTransfer with peer ID, event metadata, expected total, next offset, and a bounded byte buffer. Its API is:

~~~rust
impl IncomingImageTransfer {
    fn from_start(peer_id: &DeviceId, start: &ClipboardImageStart)
        -> Result<Self, ClipboardError>;
    fn accept_chunk(&mut self, offset: u64, data: Vec<u8>)
        -> Result<(), ClipboardError>;
    fn finish(self) -> Result<ClipboardEvent, ClipboardError>;
}
~~~

Require source/peer identity and image/png, reject zero dimensions and totals above MAX_IMAGE_BYTES, require contiguous offsets, cap each chunk at CLIPBOARD_IMAGE_CHUNK_BYTES, use checked arithmetic, verify exact total length/content hash, and validate PNG dimensions at completion.

Store partial images by (DeviceId, event_id) in ClipboardService. Add start/chunk/complete handlers and handle_peer_disconnected. A new start for the same key replaces the old one. Complete removes the partial state only after taking ownership, then calls the existing handle_remote so deduplication precedes clipboard write.

- [ ] Step 4: Implement outbound chunking and route messages.

Add:

~~~rust
async fn send_clipboard_event(
    &self,
    peer_id: &DeviceId,
    event: &ClipboardEvent,
) -> Result<(), TransportError>;
~~~

Send text as one Clipboard. For images send ClipboardImageStart, 64 KiB ClipboardImageChunk messages with increasing offsets, and ClipboardImageComplete. Every chunk keeps the original event ID/source metadata and remains below the 8 MiB frame limit.

Extend the transport-message match in lib.rs for the three variants and call the clipboard handlers. Call clipboard_events.handle_peer_disconnected(&peer.id) in the existing disconnect branch.

- [ ] Step 5: Run focused tests and commit.

~~~powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all
cargo test --manifest-path src-tauri/Cargo.toml clipboard::tests transport::tests -- --nocapture
git add src-tauri/src/clipboard.rs src-tauri/src/transport.rs src-tauri/src/protocol.rs src-tauri/src/lib.rs
git commit -m "fix: stream large clipboard images"
~~~

Expected: clipboard and transport suites pass, including an image stream larger than one frame.

---

### Task 4: Make file streaming failure-aware and non-blocking

**Files:**

- Modify: src-tauri/src/file_transfer.rs:699-949,1031-1079
- Test: src-tauri/src/file_transfer.rs

- [ ] Step 1: Write and run the failing partial-file test.

Add a helper-level test that creates a .part file with a prefix, opens it for a later chunk, and proves the prefix is preserved:

~~~rust
#[test]
fn partial_file_open_preserves_existing_prefix() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("file.part");
    fs::write(&path, b"prefix").unwrap();
    let file = open_partial_file(&path).unwrap();
    drop(file);
    assert_eq!(fs::read(&path).unwrap(), b"prefix");
}
~~~

Run:

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml file_transfer::tests::partial_file_open_preserves_existing_prefix -- --nocapture
~~~

Expected: FAIL because open_partial_file does not exist.

- [ ] Step 2: Implement explicit non-truncating writes.

Add and use:

~~~rust
fn open_partial_file(path: &Path) -> io::Result<std::fs::File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(path)
}
~~~

Replace the inline OpenOptions builder in handle_chunk; retain validated seek and write_all.

- [ ] Step 3: Write the failing receiver-abort test.

Add a real FileTransferService temporary-directory fixture and test that an invalid accepted chunk removes the incoming entry and managed staging directory. The assertion must inspect service state and filesystem, not only a mocked send call. Keep the TempDir in the fixture so paths remain alive:

~~~rust
struct TransferFixture {
    _root: tempfile::TempDir,
    service: FileTransferService,
    peer: DeviceInfo,
    accepted_transfer_id: String,
    staging_dir: PathBuf,
}

fn transfer_fixture() -> TransferFixture {
    let root = tempfile::tempdir().unwrap();
    let local = DeviceInfo::new_local("Windows Desk", 45731);
    let peer = DeviceInfo::new_local("MacBook", 45731);
    let registry = Arc::new(Mutex::new(PeerRegistry::from_paired(vec![PairedPeer {
        device: peer.clone(),
        receive_clipboard: true,
        send_clipboard: true,
        is_default_file_target: false,
        state: PeerConnectionState::Connected,
    }])));
    let (transport, _events) = TransportRuntime::new(local, registry);
    let (service, _transfer_events) = FileTransferService::new(
        Arc::new(transport),
        root.path().join("cache/active-transfers.json"),
    ).unwrap();
    let transfer_id = "transfer-1".to_string();
    let destination = root.path().join("destination");
    let staging_dir = destination.join(STAGING_DIRECTORY_NAME).join(&transfer_id);
    fs::create_dir_all(staging_dir.join("Root")).unwrap();
    let mut state = TransferStateMachine::new();
    state.offer().unwrap();
    state.start().unwrap();
    service.incoming.lock().unwrap().insert(transfer_id.clone(), IncomingTransfer {
        transfer_id: transfer_id.clone(),
        peer_id: peer.id.clone(),
        manifest: TransferManifest {
            root_name: "Root".to_string(),
            total_bytes: 4,
            entries: vec![ManifestEntry {
                relative_path: "Root/file.txt".to_string(),
                kind: ManifestEntryKind::File,
                size: 4,
            }],
        },
        destination: Some(destination),
        staging_dir: Some(staging_dir.clone()),
        received_files: HashMap::new(),
        received_bytes: 0,
        state,
    });
    TransferFixture { _root: root, service, peer, accepted_transfer_id: transfer_id, staging_dir }
}

#[tokio::test]
fn invalid_chunk_aborts_incoming_transfer_and_cleans_staging() {
    let fixture = transfer_fixture();
    let transfer_id = fixture.accepted_transfer_id.clone();
    let staging = fixture.staging_dir.clone();
    fixture.service.abort_incoming_transfer(
        &fixture.peer,
        &transfer_id,
        "invalid_file_chunk".to_string(),
    ).await.unwrap();
    assert!(!staging.exists());
    assert!(fixture.service.incoming.lock().unwrap().get(&transfer_id).is_none());
}
~~~

Import std::sync::{Arc, Mutex}, crate::domain::{DeviceInfo, PairedPeer, PeerConnectionState}, crate::registry::PeerRegistry, and crate::transport::TransportRuntime in the test module. Run the focused test and verify it fails because the abort helper is absent.

- [ ] Step 4: Implement receiver abort/cancel handling.

Add an async abort path that verifies the peer, removes the incoming transfer, marks it failed, calls cleanup_incoming, emits TransferEvent::Failed, and best-effort sends FileCancel { transfer_id, reason_code: Some(reason_code) }. Update handle_message so a FileChunk error invokes this cleanup before returning the original error. Never delete a transfer belonging to another peer.

- [ ] Step 5: Move outgoing streaming into an owned Tokio task.

Change handle_accept to transition the state and spawn:

~~~rust
let service = self.clone();
let transfer_id = accept.transfer_id;
let peer_id = peer.id.clone();
tokio::spawn(async move {
    if let Err(error) = service.finish_outgoing_transfer(peer_id, transfer_id, plan).await {
        tracing::debug!(?error, "outgoing file transfer task failed");
    }
});
Ok(())
~~~

finish_outgoing_transfer owns the existing stream, sends exactly one terminal FileComplete, removes outgoing state, clears cancellation, and emits the current terminal event. This returns the global transport-event consumer immediately so it can process cancel/disconnect messages.

- [ ] Step 6: Run tests and commit.

~~~powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all
cargo test --manifest-path src-tauri/Cargo.toml file_transfer::tests -- --nocapture
git add src-tauri/src/file_transfer.rs
git commit -m "fix: make file transfers failure-aware"
~~~

Expected: invalid receiver chunks leave no staging directory and all existing file tests remain green.

---

### Task 5: Roll back finalization and guard stale connection teardown

**Files:**

- Modify: src-tauri/src/file_transfer.rs:1250-1308
- Modify: src-tauri/src/transport.rs:506-603
- Test: both modules' test modules

- [ ] Step 1: Write the failing finalization rollback test.

Extract the final move loop behind a closure and inject a failure on the second move:

~~~rust
#[test]
fn finalization_rolls_moved_files_back_when_a_later_move_fails() {
    let root = tempfile::tempdir().unwrap();
    let staging = root.path().join("staging");
    let destination = root.path().join("destination");
    fs::create_dir_all(staging.join("Root")).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::create_dir_all(destination.join("Root")).unwrap();
    fs::write(staging.join("Root/first.txt"), b"first").unwrap();
    fs::write(staging.join("Root/second.txt"), b"second").unwrap();
    let result = move_staged_files_with(
        &destination,
        &staging,
        &["Root/first.txt", "Root/second.txt"],
        |index, staged, target| {
            if index == 1 {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected"));
            }
            fs::rename(staged, target)
        },
    );
    assert!(result.is_err());
    assert!(staging.join("Root/first.txt").is_file());
    assert!(!destination.join("Root/first.txt").exists());
}
~~~

Run the focused test and confirm it fails before the rollback helper exists.

- [ ] Step 2: Implement rollback-aware finalization.

Keep existing manifest/path/size checks. Implement move_staged_files_with to record successful (staged, destination) pairs, reverse-rename those pairs on a later error, and return the original error. Production finalization passes |_, staged, target| fs::rename(staged, target).

- [ ] Step 3: Write the failing stale-connection test.

Add the pure token-decision helper and test it before changing run_connection:

~~~rust
fn connection_token_matches(current_token: Option<&str>, ending_token: &str) -> bool {
    current_token == Some(ending_token)
}

#[test]
fn stale_connection_cannot_mark_new_connection_offline() {
    assert!(!connection_token_matches(Some("new"), "old"));
    assert!(connection_token_matches(Some("new"), "new"));
    assert!(!connection_token_matches(None, "old"));
}
~~~

Run:

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml transport::tests::stale_connection_cannot_mark_new_connection_offline -- --nocapture
~~~

Expected: FAIL against the current unconditional registry-state cleanup.

- [ ] Step 4: Guard every teardown side effect by the token.

Make the token comparison control removal from connections, offline state, and PeerDisconnected emission:

~~~rust
let is_current = self
    .connections
    .lock()
    .unwrap()
    .get(&peer.id)
    .is_some_and(|entry| entry.token == token);
if is_current {
    self.connections.lock().unwrap().remove(&peer.id);
    self.registry.lock().unwrap().set_state(&peer.id, PeerConnectionState::Offline);
    let _ = self.events.send(TransportEvent::PeerDisconnected { peer, reason_code });
}
~~~

An old connection may finish, but it must not touch current peer state.

- [ ] Step 5: Run focused tests and commit.

~~~powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all
cargo test --manifest-path src-tauri/Cargo.toml file_transfer::tests transport::tests -- --nocapture
git add src-tauri/src/file_transfer.rs src-tauri/src/transport.rs
git commit -m "fix: guard transfer finalization and reconnect cleanup"
~~~

---

### Task 6: Complete routing, documentation, and verification

**Files:**

- Modify: src-tauri/src/lib.rs:201-249 if compiler/test feedback identifies an uncovered route.
- Modify: README.md and docs/PROJECT_GUIDE.md to document the 64 MiB encoded-PNG image limit and the requirement that both peers use protocol v3.

- [ ] Step 1: Run the complete Rust suite and fix exhaustive routing errors.

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml
~~~

Expected: missing match arms for the three new clipboard messages are fixed in lib.rs; all regression tests pass.

- [ ] Step 2: Verify all required checks.

~~~powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
~~~

Expected: every command exits 0 with no warnings promoted to errors.

- [ ] Step 3: Review the final diff and tree.

~~~powershell
git diff --check
git status --short --branch
git log -8 --oneline --decorate
~~~

Expected: no whitespace errors, no unrelated files staged, and no generated build artifacts included.

- [ ] Step 4: Commit intentional documentation/routing changes.

~~~powershell
git add src-tauri/src/lib.rs README.md docs/PROJECT_GUIDE.md
git commit -m "docs: record reliable clipboard and transfer limits"
~~~

If no files changed in this step, do not create an empty commit.
