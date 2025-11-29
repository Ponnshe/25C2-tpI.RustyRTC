pub mod srtp_context;
use std::{
    io::{self, Cursor, Read, Write},
    net::{SocketAddr, UdpSocket},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    sink_log,
    tls_utils::{DTLS_CERT_PATH, DTLS_KEY_PATH, load_certs},
};

use openssl::ssl::{
    Ssl, SslContext, SslContextBuilder, SslFiletype, SslMethod, SslStream, SslVerifyMode,
};

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
}

impl BufferedUdpChannel {
    fn new(sock: Arc<UdpSocket>, peer: SocketAddr) -> Self {
        Self {
            sock,
            peer,
            reader: Cursor::new(Vec::new()),
        }
    }
}

impl Read for BufferedUdpChannel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // 1. Si hay datos pendientes en el buffer interno, entrégalos.
        let pos = self.reader.position();
        let len = self.reader.get_ref().len() as u64;

        if pos < len {
            return self.reader.read(buf);
        }

        // 2. Si el buffer está vacío, intentamos leer un nuevo paquete de la red.
        let mut recv_buf = [0u8; 4096]; // Buffer generoso para evitar truncamiento

        loop {
            match self.sock.recv_from(&mut recv_buf) {
                Ok((n, from)) => {
                    if from == self.peer {
                        // Paquete válido del peer: rellenar buffer interno
                        self.reader = Cursor::new(recv_buf[..n].to_vec());
                        // Satisfacer la lectura actual inmediatamente
                        return self.reader.read(buf);
                    }
                    // Si es de otro peer, ignorar y seguir en el loop (drenaje)
                    continue;
                }
                Err(e) => {
                    // CRÍTICO: Propagar el error (especialmente WouldBlock)
                    // No devolver Ok(0) aquí, porque eso cierra la conexión SSL.
                    return Err(e);
                }
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
fn create_base_context() -> SslContextBuilder {
    let mut builder = SslContextBuilder::new(SslMethod::dtls()).unwrap();
    builder.set_security_level(0);

    builder
        .set_tlsext_use_srtp("SRTP_AES128_CM_SHA1_80")
        .unwrap();

    // Ciphers permisivos
    builder.set_cipher_list("DEFAULT:@SECLEVEL=0").unwrap();
    builder.set_verify(SslVerifyMode::NONE);

    builder
}

pub fn run_dtls_handshake(
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
    role: DtlsRole,
    logger: Arc<dyn LogSink>,
) -> io::Result<SrtpSessionConfig> {
    // Drenaje inicial defensivo
    sock.set_nonblocking(true).ok();
    let mut drain_buf = [0u8; 4096];
    loop {
        match sock.recv_from(&mut drain_buf) {
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    // Ahora FORZAMOS blocking durante el handshake
    sock.set_nonblocking(false)
        .expect("Failed to set blocking mode for DTLS handshake");

    sink_log!(
        &logger,
        LogLevel::Info,
        "[DTLS] starting handshake as {:?}",
        role
    );

    // Intento único: crear el channel y dejar que OpenSSL bloquee hasta completar el handshake
    let channel = BufferedUdpChannel::new(sock.clone(), peer);

    let dtls_stream = match role {
        DtlsRole::Client => dtls_connect_openssl(channel),
        DtlsRole::Server => dtls_accept_openssl(channel),
    }
    .map_err(|e| {
        sink_log!(&logger, LogLevel::Error, "OpenSSL Handshake error: {:?}", e);
        io::Error::new(io::ErrorKind::Other, format!("Handshake failed: {:?}", e))
    })?;

    sock.set_nonblocking(true).ok();
    let cfg = derive_srtp_keys(&dtls_stream, role)?;
    sink_log!(&logger, LogLevel::Info, "[DTLS] Handshake Success!");
    Ok(cfg)
}
fn dtls_connect_openssl(
    stream: BufferedUdpChannel,
) -> Result<SslStream<BufferedUdpChannel>, openssl::ssl::HandshakeError<BufferedUdpChannel>> {
    let mut builder = create_base_context();

    // 1. Cargar certificados
    let server_cert_der =
        load_certs(DTLS_CERT_PATH).expect("Failed to load server cert for pinning");

    for cert_der in server_cert_der {
        let x509 = openssl::x509::X509::from_der(&cert_der.to_vec()).unwrap();
        builder.cert_store_mut().add_cert(x509).unwrap();
    }

    let ssl = Ssl::new(&builder.build()).unwrap();
    ssl.connect(stream)
}

fn dtls_accept_openssl(
    stream: BufferedUdpChannel,
) -> Result<SslStream<BufferedUdpChannel>, openssl::ssl::HandshakeError<BufferedUdpChannel>> {
    let mut builder = create_base_context();

    // Cargar identidad del servidor
    builder.set_certificate_chain_file(DTLS_CERT_PATH).unwrap();
    builder
        .set_private_key_file(DTLS_KEY_PATH, SslFiletype::PEM)
        .unwrap();
    builder
        .check_private_key()
        .expect("Private key does not match certificate");

    let ssl = Ssl::new(&builder.build()).unwrap();
    ssl.accept(stream)
}

fn derive_srtp_keys(
    stream: &SslStream<BufferedUdpChannel>,
    role: DtlsRole,
) -> io::Result<SrtpSessionConfig> {
    let selected_profile = stream.ssl().selected_srtp_profile().ok_or(io::Error::new(
        io::ErrorKind::Other,
        "No SRTP profile negotiated",
    ))?;

    // 3. CORRECCIÓN: name() devuelve &str directo
    let profile_name = selected_profile.name();

    let profile = match profile_name {
        "SRTP_AES128_CM_SHA1_80" => SrtpProfile::Aes128CmHmacSha1_80,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Unsupported SRTP profile: {}", profile_name),
            ));
        }
    };

    let label = "EXTRACTOR-dtls_srtp";
    let key_len = 16;
    let salt_len = 14;
    let total_len = 2 * (key_len + salt_len);

    let mut key_mat = vec![0u8; total_len];
    stream
        .ssl()
        .export_keying_material(&mut key_mat, label, None)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Key export failed: {}", e)))?;

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
