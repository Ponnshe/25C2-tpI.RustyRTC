# rustyrtc

A small, WebRTC/RTC engine written in Rust. It includes SDP parsing, ICE, RTP/RTCP, a media pipeline (encode/decode/packetize/depacketize), a camera manager, a logger, and a minimal GUI app for local testing.

> **Status:** actively evolving; latest stable slice includes the modules listed below.

## Features (high level)

* **SDP module** – parse/build offers & answers, rtpmap/fmtp helpers.
* **ICE module** – role selection, ufrag/pwd, candidate handling, pairing.
* **RTP module** – packet/headers, payload handling (H.264), packetizer.
* **RTCP module** – SR/RR, NACK/PLI basics.
* **RTP Session module** – SSRC streams, jitter/seq, (de)packetization glue.
* **Camera Manager** – capture frames from a local camera.
* **Connection Manager** – wiring between signaling, ICE, and media.
* **Media Agent** – encode/decode + track management (H.264, access units).
* **Core module** – events bus, session orchestration.
* **Logger** – bounded, non-blocking channel + simple macros.
* **App/GUI module** – minimal desktop app to exercise the engine.

---

## Quickstart

### Prerequisites

* Rust (stable) — install via [https://rustup.rs](https://rustup.rs)
* Linux/macOS/Windows are fine.
* H.264 via OpenH264.
* System OpenCV/Clang/LLVM dev packages.

### Build & Run

```bash
# clone
git clone https://github.com/taller-1-fiuba-rust/25C2-rustyrtc && cd rustyrtc

# build
cargo build

# run the GUI app 
cargo run
```


> If your platform doesn’t have prebuilt binaries for OpenH264/OpenCV, you may need to install those libs from your package manager first.

---

## Testing

Run the whole test suite:

```bash
cargo test
```

Lint with Clippy (treat warnings as errors) and check formatting:

```bash
cargo clippy
cargo fmt --all -- --check
```

Run a single test (example):

```bash
cargo test logger_handle -- --nocapture
```

---

## Project layout (overview)

```
src/
  app/             # App/GUI module (desktop harness, demo UI, logger)
  core/            # Core events, session orchestration
  connection_manager/
  ice_agent/       # ICE module
  media_agent/     # Media Agent (encode/decode, tracks)
  rtp/             # RTP packet, header, packetizer
  rtcp/            # RTCP packets (SR/RR/PLI/NACK)
  rtp_session/     # RTP session management (recv/send streams)
  sdp/             # SDP parse/build helpers
  camera_manager/  # Camera Manager
```

---

## Developer roster 

| Name           | Email         |
| -------------- | ---------------------- |
| *Tom Pinargote* | tpinargote@fi.uba.ar |
| *Nervo Olalla* | nolalla@fi.uba.ar |
| *Alexander Villa Jimenez* | avilla@fi.uba.ar |
| *Nico Cruz* | ncruz@fi.uba.ar |

---

