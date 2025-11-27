use std::{
    fs::File,
    io::{self, BufReader, Cursor},
    sync::Arc,
};

use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer},
};
use rustls_pemfile::{certs, pkcs8_private_keys};

/// Pinned CA used to authenticate the signaling server.
///
/// This should be the mkcert root CA, copied into the repo,
/// `certs/rootCA.pem`.
///
const MKCERT_CA_PEM: &[u8] = include_bytes!("../../certs/rootCA.pem");

/// Build a RootCertStore that trusts ONLY the pinned mkcert CA.
///
/// No system / OS roots, no webpki_roots — this is fully pinned.
fn build_pinned_root_store() -> io::Result<RootCertStore> {
    let mut root_store = RootCertStore::empty();

    let mut cursor = Cursor::new(MKCERT_CA_PEM);

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

/// Helper: load server cert chain from PEM file.
///
/// `path` should usually be the mkcert-generated `certs/signal.internal.pem`.
fn load_server_certs(path: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("opening cert {path}: {e}")))?;

    let mut reader = BufReader::new(file);

    let certs: Vec<CertificateDer<'static>> =
        certs(&mut reader).collect::<Result<_, _>>().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid server certs: {e}"),
            )
        })?;

    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "server cert file did not contain any certificates",
        ));
    }

    Ok(certs)
}

/// Helper: load server private key (PKCS#8) from PEM file.
///
/// `path` should usually be the mkcert-generated `certs/signal.internal-key.pem`.
fn load_server_key(path: &str) -> io::Result<PrivateKeyDer<'static>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut keys = pkcs8_private_keys(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid server key: {e}"),
            )
        })?;

    let key = keys
        .pop()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no private keys found"))?;

    Ok(PrivateKeyDer::from(key))
}

/// ServerConfig for the signaling server, using *no* client auth, with our mkcert-issued cert.
///
/// We’ll call this once at startup, then re-use the Arc<ServerConfig>
/// for each accepted TCP connection (wrapping in `ServerConnection` / `StreamOwned` later).
pub fn build_signaling_server_config(
    cert_path: &str,
    key_path: &str,
) -> io::Result<Arc<ServerConfig>> {
    let certs = load_server_certs(cert_path)?;
    let key = load_server_key(key_path)?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("TLS config error: {e}"))
        })?;

    Ok(Arc::new(config))
}
