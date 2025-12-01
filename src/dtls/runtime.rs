use crate::{
    config::Config,
    dtls::{
        buffered_udp_channel::BufferedUdpChannel, dtls_error::DtlsError, dtls_role::DtlsRole,
        socket_blocking_guard::SocketBlockingGuard,
    },
    log::log_sink::LogSink,
    sink_debug, sink_error, sink_info, sink_trace, sink_warn,
    srtp::{SrtpEndpointKeys, SrtpProfile, SrtpSessionConfig},
    tls_utils::{DTLS_CERT_PATH, DTLS_KEY_PATH},
};
use openssl::ssl::{HandshakeError, Ssl, SslContextBuilder, SslFiletype, SslMethod, SslStream};
use std::{
    io::{self},
    net::{SocketAddr, UdpSocket},
    sync::Arc,
    time::Duration,
};

use openssl::hash::MessageDigest;
use openssl::ssl::SslVerifyMode;

// -----------------------------------------------------------------------------
// HANDSHAKE
// -----------------------------------------------------------------------------

pub fn run_dtls_handshake(
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
    role: DtlsRole,
    logger: Arc<dyn LogSink>,
    timeout: Duration,
    expected_fingerprint: Option<String>,
    config: Arc<Config>,
) -> Result<SrtpSessionConfig, DtlsError> {
    // Draining socket (nonblocking)
    sock.set_nonblocking(true).ok();
    let mut drain_buf = [0u8; 4096];
    let mut drained_count = 0;
    while sock.recv_from(&mut drain_buf).is_ok() {
        drained_count += 1;
    }
    if drained_count > 0 {
        sink_debug!(
            &logger,
            "[DTLS] Drained {} stale packets before handshake",
            drained_count
        );
    }

    // Guard: pone socket en blocking y configura read timeout
    let _guard = SocketBlockingGuard::new(sock.clone(), Some(timeout)).map_err(DtlsError::from)?;

    sink_info!(
        &logger,
        "[DTLS] Starting handshake with {} as {:?}. Timeout: {:?}",
        peer,
        role,
        timeout
    );

    if let Some(fp) = &expected_fingerprint {
        sink_debug!(&logger, "[DTLS] Expecting remote fingerprint: {}", fp);
    } else {
        sink_warn!(
            &logger,
            "[DTLS] No remote fingerprint provided. Verification will be disabled (INSECURE for WebRTC)."
        );
    }

    let channel = BufferedUdpChannel::new(sock.clone(), peer, logger.clone());

    // Llamada al handshake
    let dtls_stream = match role {
        DtlsRole::Client => {
            dtls_connect_openssl(logger.clone(), channel, expected_fingerprint, config)
        }
        DtlsRole::Server => {
            dtls_accept_openssl(logger.clone(), channel, expected_fingerprint, config)
        }
    }
    .map_err(|e| {
        sink_error!(&logger, "[DTLS] Handshake FAILED with {}: {}", peer, e);
        e
    })?;

    // Exportación de llaves
    let cfg = derive_srtp_keys(&dtls_stream, role, logger.clone()).map_err(|e| {
        sink_error!(&logger, "[DTLS] Key derivation failed: {}", e);
        e
    })?;

    sink_info!(&logger, "[DTLS] Handshake Success! SRTP keys derived.");
    Ok(cfg)
}

fn dtls_connect_openssl(
    logger: Arc<dyn LogSink>,
    stream: BufferedUdpChannel,
    expected_fingerprint: Option<String>,
    config: Arc<Config>,
) -> Result<SslStream<BufferedUdpChannel>, DtlsError> {
    sink_debug!(&logger, "[DTLS] Client: Initializing OpenSSL context...");
    let mut builder =
        create_base_context(logger.clone(), expected_fingerprint).map_err(DtlsError::from)?;

    let cert_path = config.get_non_empty_or_default("TLS", "dtls_cert", "certs/dtls/cert.pem");
    let key_path = config.get_non_empty_or_default("TLS", "dtls_key", "certs/dtls/key.pem");

    sink_debug!(
        &logger,
        "[DTLS] Client: Loading identity (chain {} and key {})",
        cert_path,
        key_path
    );

    builder
        .set_certificate_chain_file(cert_path)
        .map_err(|e| DtlsError::Ssl(format!("set_certificate_chain_file failed: {}", e)))?;

    builder
        .set_private_key_file(key_path, SslFiletype::PEM)
        .map_err(|e| DtlsError::Ssl(format!("set_private_key_file failed: {}", e)))?;
    builder
        .check_private_key()
        .map_err(|e| DtlsError::Ssl(format!("Private key does not match certificate: {}", e)))?;

    let ssl = Ssl::new(&builder.build())
        .map_err(|e| DtlsError::Ssl(format!("Ssl::new failed: {}", e)))?;

    sink_debug!(&logger, "[DTLS] Client: Starting connect()...");
    match ssl.connect(stream) {
        Ok(s) => Ok(s),
        Err(he) => Err(handshake_error_to_dtlserr(he)),
    }
}

fn dtls_accept_openssl(
    logger: Arc<dyn LogSink>,
    stream: BufferedUdpChannel,
    expected_fingerprint: Option<String>,
    config: Arc<Config>,
) -> Result<SslStream<BufferedUdpChannel>, DtlsError> {
    sink_debug!(&logger, "[DTLS] Server: Initializing OpenSSL context...");
    let mut builder =
        create_base_context(logger.clone(), expected_fingerprint).map_err(DtlsError::from)?;

    let cert_path = config.get_non_empty_or_default("TLS", "dtls_cert", DTLS_CERT_PATH);
    let key_path = config.get_non_empty_or_default("TLS", "dtls_key", DTLS_KEY_PATH);

    sink_debug!(
        &logger,
        "[DTLS] Server: Loading chain {} and key {}",
        cert_path,
        key_path
    );

    builder
        .set_certificate_chain_file(cert_path)
        .map_err(|e| DtlsError::Ssl(format!("set_certificate_chain_file failed: {}", e)))?;

    builder
        .set_private_key_file(key_path, SslFiletype::PEM)
        .map_err(|e| DtlsError::Ssl(format!("set_private_key_file failed: {}", e)))?;

    builder
        .check_private_key()
        .map_err(|e| DtlsError::Ssl(format!("Private key does not match certificate: {}", e)))?;

    let ssl = Ssl::new(&builder.build())
        .map_err(|e| DtlsError::Ssl(format!("Ssl::new failed: {}", e)))?;

    sink_debug!(&logger, "[DTLS] Server: Starting accept()...");
    match ssl.accept(stream) {
        Ok(s) => Ok(s),
        Err(he) => Err(handshake_error_to_dtlserr(he)),
    }
}

fn derive_srtp_keys(
    stream: &SslStream<BufferedUdpChannel>,
    role: DtlsRole,
    logger: Arc<dyn LogSink>,
) -> Result<SrtpSessionConfig, DtlsError> {
    let selected_profile = stream
        .ssl()
        .selected_srtp_profile()
        .ok_or(DtlsError::NoSrtpProfile)?;

    let profile_name = selected_profile.name();
    sink_debug!(&logger, "[DTLS] Negotiated SRTP Profile: {}", profile_name);

    let profile = match profile_name {
        "SRTP_AES128_CM_SHA1_80" => SrtpProfile::Aes128CmHmacSha1_80,
        _ => {
            sink_warn!(
                &logger,
                "[DTLS] Unknown SRTP Profile selected: {}",
                profile_name
            );
            return Err(DtlsError::NoSrtpProfile);
        }
    };

    let label = "EXTRACTOR-dtls_srtp";
    let key_len = 16usize;
    let salt_len = 14usize;
    let total_len = 2 * (key_len + salt_len);

    let mut key_mat = vec![0u8; total_len];
    stream
        .ssl()
        .export_keying_material(&mut key_mat, label, None)
        .map_err(|e| DtlsError::KeyExport(format!("{}", e)))?;

    sink_trace!(
        &logger,
        "[DTLS] Key material exported successfully ({} bytes)",
        total_len
    );

    let (client_key, rest) = key_mat.split_at(key_len);
    let (server_key, rest) = rest.split_at(key_len);
    let (client_salt, rest) = rest.split_at(salt_len);
    let (server_salt, _) = rest.split_at(salt_len);

    let client_keys = SrtpEndpointKeys {
        master_key: client_key.to_vec(),
        master_salt: client_salt.to_vec(),
    };
    let server_keys = SrtpEndpointKeys {
        master_key: server_key.to_vec(),
        master_salt: server_salt.to_vec(),
    };

    let (outbound, inbound) = match role {
        DtlsRole::Client => (client_keys, server_keys),
        DtlsRole::Server => (server_keys, client_keys),
    };

    Ok(SrtpSessionConfig {
        profile,
        outbound,
        inbound,
    })
}

fn create_base_context(
    logger: Arc<dyn LogSink>,
    expected_fingerprint: Option<String>,
) -> io::Result<SslContextBuilder> {
    let mut builder = SslContextBuilder::new(SslMethod::dtls())
        .map_err(|e| io::Error::other(format!("OpenSSL init failed: {}", e)))?;

    builder
        .set_tlsext_use_srtp("SRTP_AES128_CM_SHA1_80")
        .map_err(|e| io::Error::other(format!("set_tlsext_use_srtp failed: {}", e)))?;

    builder
        .set_cipher_list("DEFAULT:@SECLEVEL=0")
        .map_err(|e| io::Error::other(format!("set_cipher_list failed: {}", e)))?;

    if let Some(fp) = expected_fingerprint {
        let logger_cb = logger.clone();

        // Enforce that a peer certificate is present
        builder.set_verify(SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);

        builder.set_verify_callback(
            SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT,
            move |_preverify_ok, ctx| {
                let cert = match ctx.current_cert() {
                    Some(c) => c,
                    None => {
                        sink_warn!(
                            logger_cb,
                            "[DTLS] Verify: No certificate presented by peer!"
                        );
                        return false;
                    }
                };

                let digest_bytes = match cert.digest(MessageDigest::sha256()) {
                    Ok(d) => d,
                    Err(e) => {
                        sink_error!(logger_cb, "[DTLS] Verify: Failed to compute digest: {}", e);
                        return false;
                    }
                };

                let computed_fp = digest_bytes
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<String>>()
                    .join(":");

                // Perform case-insensitive comparison
                if computed_fp.eq_ignore_ascii_case(&fp) {
                    sink_info!(
                        logger_cb,
                        "[DTLS] Verify: Fingerprint MATCHED ({})",
                        computed_fp
                    );
                    true
                } else {
                    sink_warn!(
                        logger_cb,
                        "[DTLS] Verify: Fingerprint MISMATCH!\n  Expected: {}\n  Got:      {}",
                        fp,
                        computed_fp
                    );
                    false
                }
            },
        );
    } else {
        builder.set_verify(SslVerifyMode::NONE);
    }

    Ok(builder)
}

/// Convierte un HandshakeError a DtlsError con mensaje útil.
fn handshake_error_to_dtlserr<E: std::fmt::Debug>(he: HandshakeError<E>) -> DtlsError {
    match he {
        HandshakeError::WouldBlock(_) => DtlsError::Handshake("Handshake would block".into()),
        HandshakeError::Failure(s) => DtlsError::Handshake(format!("{:?}", s.into_error())),
        HandshakeError::SetupFailure(e) => DtlsError::Ssl(format!("{:?}", e)),
    }
}
