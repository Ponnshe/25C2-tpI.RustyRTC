pub mod srtp_context;
use core::fmt;
use std::{
    io::{self, Cursor, Read, Write},
    net::{SocketAddr, UdpSocket},
    sync::Arc,
    time::Duration,
};

use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    sink_log,
    tls_utils::{DTLS_CERT_PATH, DTLS_KEY_PATH, load_certs},
};

use openssl::{
    error::ErrorStack,
    ssl::{HandshakeError, Ssl, SslContextBuilder, SslFiletype, SslMethod, SslStream},
    x509::X509,
};

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
/// Convierte un HandshakeError a DtlsError con mensaje útil.
/// Nota: para WouldBlock devolvemos un variant Handshake con el string —
/// en sitios donde queramos la semántica WouldBlock, mapear explícitamente.
fn handshake_error_to_dtlserr<E: std::fmt::Debug>(he: HandshakeError<E>) -> DtlsError {
    match he {
        HandshakeError::WouldBlock(_) => DtlsError::Handshake("Handshake would block".into()),
        HandshakeError::Failure(s) => DtlsError::Handshake(format!("{:?}", s.into_error())),
        HandshakeError::SetupFailure(e) => DtlsError::Ssl(format!("{:?}", e)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DtlsRole {
    Client,
    Server,
}

#[derive(Debug, Clone)]
pub struct SrtpEndpointKeys {
    pub master_key: Vec<u8>,
    pub master_salt: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub enum SrtpProfile {
    Aes128CmHmacSha1_80,
}

#[derive(Debug, Clone)]
pub struct SrtpSessionConfig {
    pub profile: SrtpProfile,
    pub outbound: SrtpEndpointKeys,
    pub inbound: SrtpEndpointKeys,
}
#[derive(Debug, Clone)]
struct BufferedUdpChannel {
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
    reader: Cursor<Vec<u8>>,
    recv_buf: Vec<u8>,
}

impl BufferedUdpChannel {
    fn new(sock: Arc<UdpSocket>, peer: SocketAddr) -> Self {
        Self {
            sock,
            peer,
            reader: Cursor::new(Vec::new()),
            recv_buf: vec![0u8; 4096],
        }
    }
}

impl Read for BufferedUdpChannel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // entrega datos pendientes
        let pos = self.reader.position();
        if pos < self.reader.get_ref().len() as u64 {
            return self.reader.read(buf);
        }

        // buffer vacío: leer del socket
        loop {
            match self.sock.recv_from(&mut self.recv_buf) {
                Ok((n, from)) => {
                    if from == self.peer {
                        // reusar parte del recv_buf sin alocar extra
                        self.reader = Cursor::new(self.recv_buf[..n].to_vec());
                        return self.reader.read(buf);
                    } else {
                        // paquete de otro peer: ignorar (o loguear)
                        // sink_log!(..., LogLevel::Debug, "[DTLS] packet from other peer: {:?}", from);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Err(io::Error::from(io::ErrorKind::WouldBlock));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Write for BufferedUdpChannel {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sock.send_to(buf, self.peer)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// LÓGICA DE HANDSHAKE
// -----------------------------------------------------------------------------

/// Configuración común para Cliente y Servidor para evitar mismatches.

pub fn run_dtls_handshake(
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
    role: DtlsRole,
    logger: Arc<dyn LogSink>,
    timeout: Duration,
) -> Result<SrtpSessionConfig, DtlsError> {
    // Drenaje inicial defensivo (nonblocking)
    sock.set_nonblocking(true).ok();
    let mut drain_buf = [0u8; 4096];
    loop {
        match sock.recv_from(&mut drain_buf) {
            Ok(_) => continue,
            Err(_) => break,
        }
    }

    // Forzamos blocking para handshake (puedes cambiar a nonblocking + handshake_with_timeout)
    sock.set_nonblocking(false).map_err(|e| {
        DtlsError::Io(io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to set blocking mode: {}", e),
        ))
    })?;

    sink_log!(
        &logger,
        LogLevel::Info,
        "[DTLS] starting handshake as {:?}",
        role
    );

    let channel = BufferedUdpChannel::new(sock.clone(), peer);

    let dtls_stream = match role {
        DtlsRole::Client => dtls_connect_openssl(channel)?,
        DtlsRole::Server => dtls_accept_openssl(channel)?,
    };

    sock.set_nonblocking(true).ok();

    let cfg = derive_srtp_keys(&dtls_stream, role).map_err(|e| DtlsError::from(e))?; // ver abajo
    sink_log!(&logger, LogLevel::Info, "[DTLS] Handshake Success!");

    Ok(cfg)
}

fn dtls_connect_openssl(
    stream: BufferedUdpChannel,
) -> Result<SslStream<BufferedUdpChannel>, DtlsError> {
    let mut builder = create_base_context().map_err(DtlsError::from)?;

    let server_cert_der = load_certs(DTLS_CERT_PATH).map_err(|e| {
        DtlsError::Io(io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to load certs: {}", e),
        ))
    })?;

    for cert_der in server_cert_der {
        let x509 = X509::from_der(&cert_der.to_vec())
            .map_err(|e| DtlsError::Ssl(format!("Invalid cert DER: {}", e)))?;
        builder
            .cert_store_mut()
            .add_cert(x509)
            .map_err(|e| DtlsError::Ssl(format!("Failed to add cert to store: {}", e)))?;
    }

    let ssl = Ssl::new(&builder.build())
        .map_err(|e| DtlsError::Ssl(format!("Ssl::new failed: {}", e)))?;

    match ssl.connect(stream) {
        Ok(s) => Ok(s),
        Err(he) => Err(handshake_error_to_dtlserr(he)),
    }
}

fn dtls_accept_openssl(
    stream: BufferedUdpChannel,
) -> Result<SslStream<BufferedUdpChannel>, DtlsError> {
    let mut builder = create_base_context().map_err(DtlsError::from)?;

    builder
        .set_certificate_chain_file(DTLS_CERT_PATH)
        .map_err(|e| DtlsError::Ssl(format!("set_certificate_chain_file failed: {}", e)))?;

    builder
        .set_private_key_file(DTLS_KEY_PATH, SslFiletype::PEM)
        .map_err(|e| DtlsError::Ssl(format!("set_private_key_file failed: {}", e)))?;

    builder
        .check_private_key()
        .map_err(|e| DtlsError::Ssl(format!("Private key does not match certificate: {}", e)))?;

    let ssl = Ssl::new(&builder.build())
        .map_err(|e| DtlsError::Ssl(format!("Ssl::new failed: {}", e)))?;

    match ssl.accept(stream) {
        Ok(s) => Ok(s),
        Err(he) => Err(handshake_error_to_dtlserr(he)),
    }
}
fn derive_srtp_keys(
    stream: &SslStream<BufferedUdpChannel>,
    role: DtlsRole,
) -> Result<SrtpSessionConfig, DtlsError> {
    let selected_profile = stream
        .ssl()
        .selected_srtp_profile()
        .ok_or(DtlsError::NoSrtpProfile)?;

    let profile_name = selected_profile.name();
    let profile = match profile_name {
        "SRTP_AES128_CM_SHA1_80" => SrtpProfile::Aes128CmHmacSha1_80,
        _ => return Err(DtlsError::NoSrtpProfile),
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

fn create_base_context() -> io::Result<SslContextBuilder> {
    let mut builder = SslContextBuilder::new(SslMethod::dtls())
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("OpenSSL init failed: {}", e)))?;

    builder
        .set_tlsext_use_srtp("SRTP_AES128_CM_SHA1_80")
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("set_tlsext_use_srtp failed: {}", e),
            )
        })?;

    // Ciphers permisivos
    builder
        .set_cipher_list("DEFAULT:@SECLEVEL=0")
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("set_cipher_list failed: {}", e),
            )
        })?;

    Ok(builder)
}
