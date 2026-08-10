# Clipboard and File Transfer Reliability Design

**Date:** 2026-08-10

**Status:** Approved for implementation

## Goal

Make external button-driven clipboard copies reliable, allow large screenshots to cross the LAN, and make file transfers fail deterministically with cleanup instead of leaving the two devices in inconsistent states.

## Context and root causes

The current Windows clipboard watcher reacts to `WM_CLIPBOARDUPDATE` and reads the clipboard exactly once. Web and desktop applications commonly populate the clipboard asynchronously or hold the clipboard open briefly, so the one read can observe a transient error or an incomplete format and the event is discarded.

Images are currently carried as uncompressed RGBA bytes inside one JSON message. The local image limit is 4 MiB, and the framed transport limit is 8 MiB. Base64 and JSON overhead make the effective wire limit smaller than the local limit, so high-resolution screenshots fail before they can be sent.

File-transfer streaming runs inline in the global transport-event consumer. While a large transfer is being streamed, that consumer cannot process cancellation, disconnect cleanup, or messages from another peer. Receiver-side chunk errors are logged but are not sent back to the sender, so the sender can report success even when the receiver has already failed. Transport cleanup also unconditionally marks a peer offline when any connection ends, even if a newer connection has replaced it.

## Scope and compatibility

In scope:

- bounded clipboard read retries for transient clipboard-provider races;
- PNG encoding for clipboard images;
- chunked clipboard-image transport with size, offset, and content validation;
- protocol-version bump for the new transport messages;
- asynchronous outgoing file streaming;
- receiver-to-sender cancellation on chunk/protocol/I/O failures;
- staging cleanup and finalization rollback for failed transfers;
- stale-connection cleanup that only updates state for the current connection;
- Rust regression tests for each failure mode and full build/test verification.

Out of scope:

- resumable transfers after a process restart;
- end-to-end encryption;
- clipboard history;
- UI redesign or new transfer controls;
- changing the existing pairing or discovery workflow.

The business protocol version changes from 2 to 3. An updated application will reject an older peer during handshake rather than exchanging incompatible clipboard messages. Both devices must run the updated build to use the repaired synchronization path.

## Design

### 1. Clipboard reads and image representation

`ClipboardService` will call a retrying blocking read helper after every Windows clipboard-change notification. The helper will make a small fixed number of attempts, wait briefly between attempts, and return the last real error only after the retry budget is exhausted. Empty/temporarily unavailable clipboard formats will also be retried. The existing content-hash tracker remains the loop-prevention mechanism.

The local `arboard` image is raw RGBA. Before creating a wire event it will be encoded as PNG, with the existing width and height retained. Image payloads will use `mime_type = "image/png"`; receiving code will decode PNG back to RGBA before calling `arboard::Clipboard::set_image`. The encoded payload will have a bounded maximum of 64 MiB and the decoded/local raw image will have a separate defensive size and dimension validation.

Text continues to use the existing single `Clipboard` message. Images use three new transport messages:

- `ClipboardImageStart`: event identity, source identity, sequence/timestamp, content hash, dimensions, MIME type, and total encoded byte count;
- `ClipboardImageChunk`: event identity, byte offset, and at most 64 KiB of base64-serialized data;
- `ClipboardImageComplete`: event identity marking the end of the stream.

The transport frame limit remains 8 MiB because every image chunk is far below that limit. The receiver stores one bounded in-progress image per event, requires strictly contiguous offsets, verifies the final length and content hash, decodes the PNG, and only then records the event and writes the system clipboard. Invalid or abandoned image streams are removed on peer disconnect and when their transfer identity is replaced.

### 2. File-transfer lifecycle

Accepting a file offer will transition the outgoing state and spawn an owned Tokio task for the streaming loop. The transport-event consumer will return immediately, so it can continue processing cancellation, disconnect, and other peer messages while file bytes are being sent. The task will preserve per-peer message ordering through the existing bounded transport sender and will send one terminal `FileComplete` message after the stream finishes.

The receiver will keep the current staging-directory model. A chunk with an unknown transfer/path, wrong peer, wrong offset, excessive size, or an overrun will cause the active incoming transfer to be removed, its staging directory to be cleaned, a failed event to be emitted, and a `FileCancel` with a stable reason code to be sent to the sender. The sender checks cancellation between chunks, stops promptly, cleans its in-memory state, and reports cancellation/failure rather than success.

The partial-file open mode will explicitly preserve existing bytes while seeking to the validated offset. Completion will continue to validate every manifest file size before finalization. If moving a multi-file staging tree fails after some files have moved, the finalizer will attempt to move already-moved files back into staging before the caller performs normal cleanup, preventing a known failure from intentionally leaving a partially materialized transfer.

### 3. Transport connection state

Connection teardown will first compare the ending connection token with the current registry entry. Only the current connection may remove the peer connection, mark the peer offline, or emit the disconnect event. An older connection ending after a reconnect will be ignored for current-state cleanup.

### 4. Error handling and observability

Existing Tauri error propagation and transfer-event shapes remain unchanged. Stable failure reason codes will be used where the current `FileCancel`/`FileComplete` fields already support them. Low-level errors will continue to be logged with the peer and transfer identifiers; the receiver will no longer silently leave an active transfer after a rejected chunk.

## Testing strategy

Tests will be added before implementation for the following behaviors:

1. A transient clipboard read failure succeeds within the retry budget, while an exhausted retry budget returns the final error.
2. A clipboard image round-trips through PNG encoding/decoding with its dimensions and RGBA bytes intact.
3. An image larger than one transport frame is split into chunks and reassembled only when offsets, total length, and hash are valid; malformed offsets and oversized streams are rejected.
4. A receiver-side file-chunk failure cleans its staging directory and sends/records cancellation instead of allowing a false successful completion.
5. A failed multi-file finalization rolls moved files back into staging where possible.
6. A stale transport connection cannot mark a newer connection offline.

After each failing test is observed, the smallest implementation change will be made and the focused test rerun. Final verification will run `cargo fmt --manifest-path src-tauri\\Cargo.toml --all -- --check`, `cargo check --manifest-path src-tauri\\Cargo.toml`, `cargo clippy --manifest-path src-tauri\\Cargo.toml --all-targets -- -D warnings`, `cargo test --manifest-path src-tauri\\Cargo.toml`, and `pnpm build`.

## Files expected to change

- `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`: PNG codec dependency.
- `src-tauri/src/protocol.rs`: protocol version 3 expectation updates.
- `src-tauri/src/transport.rs`: clipboard image message variants and stale-connection cleanup.
- `src-tauri/src/clipboard.rs`: retrying reads, PNG conversion, chunk send/receive, and validation.
- `src-tauri/src/file_transfer.rs`: asynchronous streaming, receiver failure cancellation, and finalization rollback.
- `src-tauri/src/lib.rs`: route new clipboard image messages and peer-disconnect cleanup.
- `docs/superpowers/plans/2026-08-10-sync-reliability.md`: implementation checklist after this design review.
