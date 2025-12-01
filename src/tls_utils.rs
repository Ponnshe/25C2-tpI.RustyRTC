use crate::config::Config;
use rustls::{
    RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer},
};
use rustls_pemfile::{Item, certs, read_one};
use std::{
    fs::File,
    io::{self, BufReader, Cursor},
};

use openssl::hash::MessageDigest;
use openssl::x509::X509;

// ----------------------------------------------------------------------
// ROOT STORE AND CONSTANTS
// ----------------------------------------------------------------------
// --- Signaling Constants (Rustls / mkcert) ---
pub const SIGNALING_CA_PEM: &[u8] = include_bytes!("../certs/signaling/rootCA.pem");
pub const SIGNALING_CERT_PATH: &str = "certs/signaling/cert.pem";
pub const SIGNALING_KEY_PATH: &str = "certs/signaling/key.pem";
pub const SIGNALING_DOMAIN: &str = "signal.internal";

// --- DTLS Constants (OpenSSL / Self-signed) ---
pub const DTLS_CERT_PATH: &str = "certs/dtls/cert.pem";
pub const DTLS_KEY_PATH: &str = "certs/dtls/key.pem";
// For DTLS pinning, we use the peer's certificate as if it were the CA
pub const DTLS_CA_PATH: &str = "certs/dtls/cert.pem";
pub const DTLS_DOMAIN: &str = "dtls.internal";

/// Builds a `RootCertStore` that trusts ONLY the internal CA.
///
/// # Errors
///
/// Returns an `io::Error` if the PEM-encoded root CA is invalid or contains no certificates.
pub fn build_pinned_root_store() -> io::Result<RootCertStore> {
    let mut root_store = RootCertStore::empty();
    let mut cursor = Cursor::new(SIGNALING_CA_PEM);

    let ca_certs: Vec<CertificateDer<'static>> = certs(&mut cursor)
        .collect::<Result<_, _>>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid CA PEM: {e}")))?;

    if ca_certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mkcert CA PEM did not contain any certificates",
        ));
    }

    for cert in ca_certs {
        root_store
            .add(cert)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad CA cert: {e}")))?;
    }

    Ok(root_store)
}

// ----------------------------------------------------------------------
// CERTIFICATE AND KEY LOADING (Robust Logic)
// ----------------------------------------------------------------------

/// Loads a certificate chain from a PEM file.
///
/// # Errors
///
/// Returns an `io::Error` if the file cannot be opened or if the PEM content is invalid.
pub fn load_certs(path: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("opening cert {path}: {e}")))?;
    let mut reader = BufReader::new(file);

    let certs: Vec<CertificateDer<'static>> = certs(&mut reader)
        .collect::<Result<_, _>>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid certs: {e}")))?;

    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cert file did not contain any certificates",
        ));
    }

    Ok(certs)
}

/// Loads a private key from a PEM file.
/// Supports PKCS1, PKCS8, and Sec1 (EC) formats.
///
/// # Errors
///
/// Returns an `io::Error` if the file cannot be opened, is malformed,
/// or does not contain a valid private key.
pub fn load_private_key(path: &str) -> io::Result<PrivateKeyDer<'static>> {
    let file = File::open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("opening key {path}: {e}")))?;
    let mut reader = BufReader::new(file);

    // Iterate through PEM items until a valid key is found.
    loop {
        match read_one(&mut reader) {
            Ok(Some(Item::Pkcs1Key(key))) => return Ok(key.into()),
            Ok(Some(Item::Pkcs8Key(key))) => return Ok(key.into()),
            Ok(Some(Item::Sec1Key(key))) => return Ok(key.into()),
            Ok(None) => break, // End of file
            Ok(Some(_)) => {}  // It's a certificate or other item, ignore
            Err(e) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("key parse error: {e}"),
                ));
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("no private key found in {path}"),
    ))
}

/// # Errors
///
/// Returns `io::Error` if the certificate file path is invalid or the file cannot be read.
pub fn load_signaling_certs(config: &Config) -> io::Result<Vec<CertificateDer<'static>>> {
    let path = config.get_non_empty_or_default("TLS", "signaling_cert", "certs/signaling/cert.pem");
    load_certs(path)
}

/// # Errors
///
/// Returns `io::Error` if the key file path is invalid or the file cannot be read.
pub fn load_signaling_private_key(config: &Config) -> io::Result<PrivateKeyDer<'static>> {
    let path = config.get_non_empty_or_default("TLS", "signaling_key", "certs/signaling/key.pem");
    load_private_key(path)
}

/// # Errors
///
/// Returns `io::Error` if the certificate file path is invalid or the file cannot be read.
pub fn load_dtls_certs(config: &Config) -> io::Result<Vec<CertificateDer<'static>>> {
    let path = config.get_non_empty_or_default("TLS", "dtls_cert", "certs/dtls/cert.pem");
    load_certs(path)
}

/// # Errors
///
/// Returns `io::Error` if the key file path is invalid or the file cannot be read.
pub fn load_dtls_private_key(config: &Config) -> io::Result<PrivateKeyDer<'static>> {
    let path = config.get_non_empty_or_default("TLS", "dtls_key", "certs/dtls/key.pem");
    load_private_key(path)
}

/// Calculates the SHA-256 fingerprint of the local DTLS certificate for use in SDP.
/// Format: "XX:YY:ZZ:..." (uppercase)
///
/// # Errors
///
/// Returns `io::Error` if the certificate cannot be loaded, parsed, or if the
/// hashing operation fails.
pub fn get_local_fingerprint_sha256(config: &Config) -> std::io::Result<String> {
    let certs_der = load_dtls_certs(config)?;

    if certs_der.is_empty() {
        return Err(io::Error::other("No certs found"));
    }

    // Parse with OpenSSL
    let x509 = X509::from_der(&certs_der[0])
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Calculate SHA256 Digest
    let digest = x509
        .digest(MessageDigest::sha256())
        .map_err(io::Error::other)?;

    // Format to Hex separated by colons
    let hex: Vec<String> = digest.iter().map(|b| format!("{b:02X}")).collect();

    Ok(hex.join(":"))
}
