use core::fmt;
use std::io;

use openssl::error::ErrorStack;

#[derive(Debug)]
pub enum DtlsError {
    Io(io::Error),
    Ssl(String),       // errores de OpenSSL como string
    Handshake(String), // fallo en handshake (incluye Failure/SetupFailure)
    NoSrtpProfile,
    KeyExport(String),
}
impl fmt::Display for DtlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DtlsError::Io(e) => write!(f, "IO error: {}", e),
            DtlsError::Ssl(s) => write!(f, "OpenSSL error: {}", s),
            DtlsError::Handshake(s) => write!(f, "Handshake error: {}", s),
            DtlsError::NoSrtpProfile => write!(f, "No SRTP profile negotiated"),
            DtlsError::KeyExport(s) => write!(f, "Key export failed: {}", s),
        }
    }
}

impl From<io::Error> for DtlsError {
    fn from(e: io::Error) -> Self {
        DtlsError::Io(e)
    }
}
impl From<ErrorStack> for DtlsError {
    fn from(e: ErrorStack) -> Self {
        DtlsError::Ssl(format!("{}", e))
    }
}
