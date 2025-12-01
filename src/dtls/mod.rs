//! This module provides a DTLS implementation for WebRTC, including handshake
//! procedures, error handling, and role management.
pub mod buffered_udp_channel;
pub mod dtls_error;
pub mod dtls_role;
pub mod runtime;
pub mod socket_blocking_guard;
pub use dtls_role::DtlsRole;
pub use runtime::run_dtls_handshake;
