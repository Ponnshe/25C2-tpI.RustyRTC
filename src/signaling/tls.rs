use std::{
    io::{self, Cursor},
    sync::Arc,
};

use rustls::{ClientConfig, RootCertStore, ServerConfig, pki_types::CertificateDer};
use rustls_pemfile::certs;

use crate::tls_utils::{SIGNALING_CA_PEM, load_certs, load_private_key};

/// Build a RootCertStore that trusts ONLY the pinned mkcert CA.
fn build_pinned_root_store() -> io::Result<RootCertStore> {
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

/// ClientConfig for the signaling client, using ONLY the pinned mkcert CA.
///
/// This is what we'll pass to `SignalingClient::connect_tls`.
pub fn build_signaling_client_config() -> io::Result<Arc<ClientConfig>> {
    let root_store = build_pinned_root_store()?;

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(Arc::new(config))
}

/// ServerConfig for the signaling server, using *no* client auth, with our mkcert-issued cert.
///
/// We’ll call this once at startup, then re-use the Arc<ServerConfig>
/// for each accepted TCP connection (wrapping in `ServerConnection` / `StreamOwned` later).
pub fn build_signaling_server_config(
    cert_path: &str,
    key_path: &str,
) -> io::Result<Arc<ServerConfig>> {
    let certs = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("TLS config error: {e}"))
        })?;

    Ok(Arc::new(config))
}
