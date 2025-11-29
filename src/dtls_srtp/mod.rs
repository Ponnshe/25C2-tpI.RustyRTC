use std::{
    io::{self, Read, Write},
    net::{SocketAddr, UdpSocket},
    sync::Arc,
};

use crate::app::{log_level::LogLevel, log_sink::LogSink};
use crate::sink_log;
use crate::tls_utils::{CN_KEY_PATH, CN_PATH, CN_SERVER, load_certs, load_private_key};

use openssl::pkcs12::Pkcs12;
use openssl::pkey::PKey;
use openssl::x509::X509;

use udp_dtls::{
    Certificate, DtlsAcceptor, DtlsConnector, DtlsStream, HandshakeError, Identity, SrtpProfile,
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

#[derive(Debug, Clone)]
pub struct SrtpSessionConfig {
    pub profile: SrtpProfile,
    pub outbound: SrtpEndpointKeys,
    pub inbound: SrtpEndpointKeys,
}

struct UdpChannel {
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
}

impl Read for UdpChannel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let (n, from) = self.sock.recv_from(buf)?;
            if from == self.peer {
                return Ok(n);
            }
        }
    }
}

impl Write for UdpChannel {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.sock.send_to(buf, self.peer)?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn run_dtls_handshake(
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
    role: DtlsRole,
    logger: Arc<dyn LogSink>,
) -> io::Result<SrtpSessionConfig> {
    let channel = UdpChannel { sock, peer };

    sink_log!(
        &logger,
        LogLevel::Info,
        "[DTLS] starting handshake as {:?}",
        role
    );

    // dtls_connect y dtls_accept devuelven io::Result directamente
    let dtls_stream = match role {
        DtlsRole::Client => dtls_connect(channel, &logger)?,
        DtlsRole::Server => dtls_accept(channel, &logger)?,
    };

    let cfg = derive_srtp_from_dtls(dtls_stream, role, &logger)?;

    sink_log!(
        &logger,
        LogLevel::Info,
        "[DTLS] handshake done. Selected SRTP profile: {:?}",
        cfg.profile
    );

    Ok(cfg)
}

fn dtls_connect(
    channel: UdpChannel,
    logger: &Arc<dyn LogSink>,
) -> io::Result<DtlsStream<UdpChannel>> {
    let mut builder = DtlsConnector::builder();
    builder.add_srtp_profile(SrtpProfile::Aes128CmSha180);

    // 1. Cargar Root CA (io::Error se propaga solo con ?)
    let ca_certs_der = load_certs("certs/rootCA.pem")?;

    for cert_der in ca_certs_der {
        // 2. Convertir DER a OpenSSL X509. Mapeamos error de OpenSSL a io::Error
        let x509 = X509::from_der(&cert_der.to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        builder.add_root_certificate(Certificate(x509));
    }

    let connector = builder.build().map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("DTLS Builder error: {:?}", e))
    })?;

    sink_log!(logger, LogLevel::Debug, "[DTLS] client: calling connect()");

    // 3. Conectar y manejar HandshakeError
    match connector.connect(CN_SERVER, channel) {
        Ok(stream) => Ok(stream),
        Err(HandshakeError::Failure(e)) => Err(io::Error::new(
            io::ErrorKind::Other,
            format!("DTLS Handshake Failure: {:?}", e),
        )),
        Err(HandshakeError::WouldBlock(_)) => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "DTLS Handshake WouldBlock",
        )),
    }
}

fn dtls_accept(
    channel: UdpChannel,
    logger: &Arc<dyn LogSink>,
) -> io::Result<DtlsStream<UdpChannel>> {
    // 1. Cargar archivos
    let certs_der = load_certs(CN_PATH)?;
    let key_der = load_private_key(CN_KEY_PATH)?;

    // 2. Convertir a objetos OpenSSL (Mapeamos errores a io::Error)
    let x509 = X509::from_der(&certs_der[0].to_vec())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let pkey = PKey::private_key_from_der(key_der.secret_der())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // 3. Crear PKCS#12 usando la API nueva (build2) para evitar deprecation
    let pkcs12 = Pkcs12::builder()
        .name("main")
        .pkey(&pkey)
        .cert(&x509)
        .build2("password")
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("PKCS12 build error: {}", e)))?;

    let pkcs12_der = pkcs12
        .to_der()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    // 4. Crear Identity para udp_dtls
    // udp_dtls::Error no tiene 'new', pero implementa From<ErrorStack> implícitamente
    // Sin embargo, map_err aquí debe devolver un io::Error para ser consistente
    let identity = Identity::from_pkcs12(&pkcs12_der, "password").map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Identity creation error: {:?}", e),
        )
    })?;

    let mut builder = DtlsAcceptor::builder(identity);
    builder.add_srtp_profile(SrtpProfile::Aes128CmSha180);

    let acceptor = builder.build().map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("DTLS Acceptor error: {:?}", e),
        )
    })?;

    sink_log!(logger, LogLevel::Debug, "[DTLS] server: calling accept()");

    // 5. Aceptar y manejar HandshakeError
    match acceptor.accept(channel) {
        Ok(stream) => Ok(stream),
        Err(HandshakeError::Failure(e)) => Err(io::Error::new(
            io::ErrorKind::Other,
            format!("DTLS Handshake Failure: {:?}", e),
        )),
        Err(HandshakeError::WouldBlock(_)) => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "DTLS Handshake WouldBlock",
        )),
    }
}

fn derive_srtp_from_dtls<S: Read + Write>(
    dtls: DtlsStream<S>,
    role: DtlsRole,
    logger: &Arc<dyn LogSink>,
) -> io::Result<SrtpSessionConfig> {
    let profile = dtls
        .selected_srtp_profile()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "no SRTP profile negotiated"))?;

    let (key_len, salt_len) = srtp_key_salt_lens(profile)?;
    let total = 2 * (key_len + salt_len);

    let km = dtls
        .keying_material(total)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;

    let mut offset = 0;
    let client_mk = km[offset..offset + key_len].to_vec();
    offset += key_len;
    let server_mk = km[offset..offset + key_len].to_vec();
    offset += key_len;
    let client_ms = km[offset..offset + salt_len].to_vec();
    offset += salt_len;
    let server_ms = km[offset..offset + salt_len].to_vec();

    let (outbound, inbound) = match role {
        DtlsRole::Client => (
            SrtpEndpointKeys {
                master_key: client_mk,
                master_salt: client_ms,
            },
            SrtpEndpointKeys {
                master_key: server_mk,
                master_salt: server_ms,
            },
        ),
        DtlsRole::Server => (
            SrtpEndpointKeys {
                master_key: server_mk,
                master_salt: server_ms,
            },
            SrtpEndpointKeys {
                master_key: client_mk,
                master_salt: client_ms,
            },
        ),
    };

    Ok(SrtpSessionConfig {
        profile,
        outbound,
        inbound,
    })
}

fn srtp_key_salt_lens(profile: SrtpProfile) -> io::Result<(usize, usize)> {
    match profile {
        SrtpProfile::Aes128CmSha180 => Ok((16, 14)),
        other => Err(io::Error::new(
            io::ErrorKind::Other,
            format!("unsupported profile: {:?}", other),
        )),
    }
}
