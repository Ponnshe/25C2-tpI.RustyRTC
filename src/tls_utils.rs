use std::{
    fs::File,
    io::{self, BufReader, Cursor},
};

use rustls::{
    RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer},
};
use rustls_pemfile::{Item, certs, read_one};

// ----------------------------------------------------------------------
// ROOT STORE AND CONSTANTS
// ----------------------------------------------------------------------

/// Pinned CA used to authenticate internal services (Signaling & Media).
/// Incluimos los bytes en el binario para portabilidad.
pub const MKCERT_CA_PEM: &[u8] = include_bytes!("../certs/rootCA.pem");
pub const CN_SERVER: &str = "signal.internal";
pub const CN_PATH: &str = "certs/signal.internal.pem";
pub const CN_KEY_PATH: &str = "certs/signal.internal-key.pem";

/// Construye un RootCertStore que confía ÚNICAMENTE en la CA interna.
pub fn build_pinned_root_store() -> io::Result<RootCertStore> {
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

// ----------------------------------------------------------------------
// CARGA DE CERTIFICADOS Y LLAVES (Lógica robusta)
// ----------------------------------------------------------------------

/// Carga la cadena de certificados del servidor desde un archivo PEM.
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

/// Carga la clave privada desde un archivo PEM.
/// Soporta PKCS1, PKCS8 y Sec1 (EC).
pub fn load_private_key(path: &str) -> io::Result<PrivateKeyDer<'static>> {
    let file = File::open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("opening key {path}: {e}")))?;
    let mut reader = BufReader::new(file);

    // Iteramos sobre los items del PEM hasta encontrar una llave válida.
    loop {
        match read_one(&mut reader) {
            Ok(Some(Item::Pkcs1Key(key))) => return Ok(key.into()),
            Ok(Some(Item::Pkcs8Key(key))) => return Ok(key.into()),
            Ok(Some(Item::Sec1Key(key))) => return Ok(key.into()),
            Ok(None) => break,       // Fin del archivo
            Ok(Some(_)) => continue, // Es un certificado u otro item, ignorar
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
