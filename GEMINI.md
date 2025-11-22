# GEMINI.md

## Project Overview

This project is a WebRTC implementation in Rust, named `rustyrtc`. It is a desktop application with a graphical user interface (GUI) built using the `eframe` and `egui` libraries.

The application allows two peers to establish a WebRTC connection by manually exchanging Session Description Protocol (SDP) messages. The UI provides text areas for users to paste the remote peer's SDP and to view and copy their own local SDP.

The project defines a custom signaling protocol for establishing and tearing down connections, with messages like `SYN`, `SYN-ACK`, `ACK`, and `FIN`.

## Building and Running

### Building

To build the project, run the following command:

```bash
cargo build
```

### Running the Application

To run the application, use the following command:

```bash
cargo run
```

### Testing

To run the test suite, use the following command:

```bash
cargo test
```

## Architecture and Integration

The project is structured in several layers:
- **`app`**: Handles the GUI and user interaction.
- **`core/engine.rs`**: Acts as the central orchestrator.
- **`connection_manager`**: Manages the SDP and ICE negotiation process.
- **`core/session.rs`**: Implements the custom `SYN/ACK` application-level handshake.
- **`rtp_session`**: Implements the RTP/RTCP media transport layer.

## Development Conventions

### Linting

The project uses `clippy` for linting. To check the code for style and correctness, run:

```bash
cargo clippy --all-targets --all-features
```

The project enforces a strict set of linting rules, as defined in `Cargo.toml`.

### Code Style

The code follows standard Rust conventions. The use of `unwrap()` and `expect()` is discouraged, and the linter is configured to deny them.

## Session Summary

In this session, two major improvements were made to the media handling pipeline:

1.  **RTP Packet Reordering and Loss Handling**: A jitter buffer was implemented in `rtp_recv_stream.rs`. This buffer reorders incoming RTP packets based on their sequence numbers and handles packet loss by declaring packets lost if they don't arrive within a specified time window. This improves the robustness of the media transport.

2.  **Encoder and Packetizer Worker Threads**: The media processing was refactored to offload CPU-intensive tasks to background threads, improving the performance and responsiveness of the main application loop.
    -   An **encoder worker** (`src/media_agent/encoder_worker.rs`) was introduced to handle video frame encoding in a separate thread.
    -   A **packetizer worker** (`src/media_transport/packetizer_worker.rs`) was created to handle RTP packetization of encoded frames in another dedicated thread.
    -   The `MediaAgent` and `MediaTransport` modules were updated to orchestrate these new workers through an event-driven approach.
