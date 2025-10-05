use std::fmt;

use crate::sdp::attribute::Attribute;
use crate::sdp::bandwidth::Bandwidth;
use crate::sdp::connection::Connection;
use crate::sdp::port_spec::PortSpec;

/// Enum que representa los posibles tipos de medios en una sección `m=` de un SDP.
///
/// Los valores estándar son `Audio`, `Video`, `Text`, `Application` y `Message`.
/// Para medios no estándar se utiliza `Other(String)`.
#[derive(Debug)]
pub enum MediaKind {
    Audio,
    Video,
    Text,
    Application,
    Message,
    Other(String),
}

impl fmt::Display for MediaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MediaKind::*;
        match self {
            Audio => f.write_str("audio"),
            Video => f.write_str("video"),
            Text => f.write_str("text"),
            Application => f.write_str("application"),
            Message => f.write_str("message"),
            Other(s) => f.write_str(s),
        }
    }
}

impl From<&str> for MediaKind {
    fn from(s: &str) -> Self {
        match s {
            "audio" => MediaKind::Audio,
            "video" => MediaKind::Video,
            "text" => MediaKind::Text,
            "application" => MediaKind::Application,
            "message" => MediaKind::Message,
            other => MediaKind::Other(other.to_string()),
        }
    }
}

/// Representa una descripción de media (`m=`) dentro de un SDP.
///
/// Contiene toda la información asociada a un medio específico, incluyendo
/// puerto, protocolo, formatos, atributos y líneas extra que pueden ser necesarias
/// para round-trip parsing.
#[derive(Debug)]
pub struct Media {
    /// Tipo de medio (`Audio`, `Video`, etc.)
    kind: MediaKind,

    /// Puerto base y cantidad de puertos (rango) usando `PortSpec`.
    port: PortSpec,

    /// Protocolo de transporte (ej. `"UDP/TLS/RTP/SAVPF"`).
    proto: String,

    /// Formatos de payload o tokens asociados al medio.
    fmts: Vec<String>,

    /// Título opcional del medio (línea `i=`).
    title: Option<String>,

    /// Información de conexión específica del medio (línea `c=`).
    connection: Option<Connection>,

    /// Líneas de ancho de banda asociadas (`b=*`).
    bandwidth: Vec<Bandwidth>,

    /// Atributos del medio (`a=*`).
    attrs: Vec<Attribute>,

    /// Líneas adicionales desconocidas o no estándar para asegurar round-trip.
    extra_lines: Vec<String>,
}

impl Media {
    /// Crea una nueva instancia de `Media` con todos los campos especificados.
    ///
    /// # Parámetros
    /// - `kind`: tipo de medio (`MediaKind`).
    /// - `port`: puerto base y opcional número de puertos (`PortSpec`).
    /// - `proto`: protocolo de transporte.
    /// - `fmts`: formatos de payload.
    /// - `title`: título opcional.
    /// - `connection`: información de conexión opcional.
    /// - `bandwidth`: lista de líneas de ancho de banda.
    /// - `attrs`: lista de atributos.
    /// - `extra_lines`: líneas desconocidas para round-trip.
    ///
    /// # Ejemplo
    /// ```rust
    /// use crate::sdp::{Media, MediaKind, PortSpec, Bandwidth, Attribute};
    /// let media = Media::new(
    ///     MediaKind::Audio,
    ///     PortSpec { base: 5004, num: None },
    ///     "RTP/AVP",
    ///     vec!["0".to_string(), "96".to_string()],
    ///     None,
    ///     None,
    ///     vec![],
    ///     vec![],
    ///     vec![],
    /// );
    /// ```
    pub fn new<K: Into<String>>(
        kind: MediaKind,
        port: PortSpec,
        proto: K,
        fmts: Vec<String>,
        title: Option<String>,
        connection: Option<Connection>,
        bandwidth: Vec<Bandwidth>,
        attrs: Vec<Attribute>,
        extra_lines: Vec<String>,
    ) -> Self {
        Self {
            kind,
            port,
            proto: proto.into(),
            fmts,
            title,
            connection,
            bandwidth,
            attrs,
            extra_lines,
        }
    }

    /// Crea un `Media` por defecto con valores de placeholder.
    ///
    /// Útil para inicialización rápida o pruebas.
    pub fn new_blank() -> Self {
        Self {
            kind: MediaKind::Audio,
            port: PortSpec::new_blank(),
            proto: "".to_string(),
            fmts: Vec::new(),
            title: None,
            connection: None,
            bandwidth: Vec::new(),
            attrs: Vec::new(),
            extra_lines: Vec::new(),
        }
    }

    // --- GETTERS ---

    /// Retorna el tipo de medio (`MediaKind`) de esta sección `m=`.
    ///
    /// Ejemplo: `Audio`, `Video`, `Other("custom")`.
    pub fn kind(&self) -> &MediaKind {
        &self.kind
    }

    /// Retorna la especificación de puerto (`PortSpec`) de la media.
    /// Puede incluir base y cantidad de puertos para rangos.
    pub fn port(&self) -> &PortSpec {
        &self.port
    }

    /// Retorna el protocolo de transporte utilizado (ej. `"UDP/TLS/RTP/SAVPF"`).
    pub fn proto(&self) -> &str {
        &self.proto
    }

    /// Retorna los formatos de payload asociados a la media.
    /// Cada string representa un `<fmt>` de la línea `m=` o `a=rtpmap`.
    pub fn fmts(&self) -> &Vec<String> {
        &self.fmts
    }

    /// Retorna el título de la media (`i=`) si está presente.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Retorna la conexión asociada a la media (`c=`) si está presente.
    pub fn connection(&self) -> Option<&Connection> {
        self.connection.as_ref()
    }

    /// Retorna la lista de líneas de ancho de banda (`b=*`) asociadas a esta media.
    pub fn bandwidth(&self) -> &Vec<Bandwidth> {
        &self.bandwidth
    }

    /// Retorna los atributos (`a=*`) definidos en esta media.
    pub fn attrs(&self) -> &Vec<Attribute> {
        &self.attrs
    }

    /// Retorna cualquier línea adicional desconocida o no estándar, útil para round-trip.
    pub fn extra_lines(&self) -> &Vec<String> {
        &self.extra_lines
    }

    // --- SETTERS ---

    /// Establece el tipo de medio (`MediaKind`) de esta sección `m=`.
    pub fn set_kind(&mut self, kind: MediaKind) {
        self.kind = kind;
    }

    /// Establece la especificación de puerto (`PortSpec`) de la media.
    pub fn set_port(&mut self, port: PortSpec) {
        self.port = port;
    }

    /// Establece el protocolo de transporte de la media.
    pub fn set_proto<S: Into<String>>(&mut self, proto: S) {
        self.proto = proto.into();
    }

    /// Establece la lista de formatos de payload (`<fmt>` tokens) de la media.
    pub fn set_fmts(&mut self, fmts: Vec<String>) {
        self.fmts = fmts;
    }

    /// Establece el título de la media (`i=`), opcional.
    pub fn set_title<S: Into<String>>(&mut self, title: Option<S>) {
        self.title = title.map(|s| s.into());
    }

    /// Establece la conexión de la media (`c=`), opcional.
    pub fn set_connection(&mut self, connection: Option<Connection>) {
        self.connection = connection;
    }

    /// Establece la lista de líneas de ancho de banda (`b=*`) de la media.
    pub fn set_bandwidth(&mut self, bandwidth: Vec<Bandwidth>) {
        self.bandwidth = bandwidth;
    }

    /// Establece los atributos (`a=*`) de la media.
    pub fn set_attrs(&mut self, attrs: Vec<Attribute>) {
        self.attrs = attrs;
    }

    /// Establece cualquier línea adicional desconocida o extra para round-trip.
    pub fn set_extra_lines(&mut self, extra_lines: Vec<String>) {
        self.extra_lines = extra_lines;
    }
    /// Agrega un formato de payload (`<fmt>`) a la media.
    pub fn add_fmt<S: Into<String>>(&mut self, fmt: S) {
        self.fmts.push(fmt.into());
    }

    /// Agrega un atributo (`a=*`) a la media.
    pub fn add_attr(&mut self, attr: Attribute) {
        self.attrs.push(attr);
    }

    /// Agrega un ancho de banda (`b=*`) a la media.
    pub fn add_bandwidth(&mut self, bw: Bandwidth) {
        self.bandwidth.push(bw);
    }

    /// Agrega una línea adicional desconocida o extra para round-trip.
    pub fn add_extra_line<S: Into<String>>(&mut self, line: S) {
        self.extra_lines.push(line.into());
    }
}
