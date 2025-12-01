# rustyrtc

A robust WebRTC/RTC engine written in Rust. It includes SDP parsing, ICE, DTLS, SRTP, a full media pipeline (encode/decode/packetize/depacketize), a congestion controller, a custom signaling solution, and a GUI app for testing.

> **Status:** actively evolving; supports secure media transport and custom signaling.

## Features (High Level)

* **SDP module** – Parse/build offers & answers, rtpmap/fmtp helpers.
* **ICE module** – Connectivity checks, role selection, candidate gathering/pairing.
* **DTLS module** – Secure handshake and key derivation (wrapping OpenSSL).
* **SRTP module** – Secure Real-time Transport Protocol (AES/HMAC encryption for media).
* **Congestion Controller** – Bandwidth estimation and flow control.
* **RTP/RTCP modules** – Packet handling, headers, SR/RR reports, NACKs/PLI.
* **Media Transport** – Event loops for packetization/depacketization and media flow.
* **Signaling** – dedicated Server (`signaling_server`) and Client (`signaling_client`) implementation.
* **Camera Manager** – Capture frames from local devices via OpenCV.
* **App/GUI module** – `eframe/wgpu` based desktop app for testing calls.

---

## Quickstart

### Prerequisites

* Rust (stable) — install via [https://rustup.rs](https://rustup.rs)
* **OpenSSL** (dev packages) — Required for DTLS/SRTP.
* **OpenH264** — Required for video encoding/decoding.
* **OpenCV** — Required for camera capture.
* Clang/LLVM — Required for bindgen operations.

### Build & Run

#### 1. Build the project
```bash
# clone
git clone [https://github.com/taller-1-fiuba-rust/25C2-rustyrtc](https://github.com/taller-1-fiuba-rust/25C2-rustyrtc) && cd rustyrtc

# build (release is recommended for video performance)
cargo build --release

