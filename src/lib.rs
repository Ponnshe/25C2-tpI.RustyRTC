//! RoomRTC is a WebRTC implementation designed for local network communication.
//!
//! It provides two main binaries:
//! - `rustyrtc`: A client application with a GUI for establishing WebRTC connections.
//! - `signaling_server`: A server for handling signaling between WebRTC clients.
//!
//! The crate is structured into several modules, each responsible for a specific
//! aspect of the WebRTC protocol and application functionality.

/// Application-specific GUI components and logic.
pub mod app;
/// Manages camera access and video frame acquisition.
pub mod camera_manager;
/// Handles configuration loading and management.
pub mod config;
/// Implements congestion control algorithms for media streams.
pub mod congestion_controller;
/// Manages ICE, SDP, and DTLS negotiation for peer connections.
pub mod connection_manager;
/// Contains core WebRTC engine logic, session management, and event handling.
pub mod core;
/// DTLS (Datagram Transport Layer Security) implementation.
pub mod dtls;
/// File handler for P2P file transfer.
pub mod file_handler;
/// ICE (Interactive Connectivity Establishment) implementation for NAT traversal.
pub mod ice;
/// Logging utilities for the application.
pub mod log;
/// Handles media encoding and decoding.
pub mod media_agent;
/// Manages RTP/RTCP media transport.
pub mod media_transport;
/// RTCP (RTP Control Protocol) packet parsing and building.
pub mod rtcp;
/// RTP (Real-time Transport Protocol) packet parsing and building.
pub mod rtp;
/// Manages RTP sessions for sending and receiving media.
pub mod rtp_session;
/// SCTP implementation for file transfer.
pub mod sctp;
/// SDP (Session Description Protocol) parsing and building.
pub mod sdp;
/// Signaling server implementation for coordinating WebRTC connections.
pub mod signaling;
/// Signaling client for communicating with the signaling server.
pub mod signaling_client;
/// SRTP (Secure Real-time Transport Protocol) implementation.
pub mod srtp;
/// TLS (Transport Layer Security) utility functions.
pub mod tls_utils;
