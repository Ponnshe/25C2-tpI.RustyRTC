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

#[allow(clippy::use_self)]
impl fmt::Display for MediaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaKind::Audio => f.write_str("audio"),
            MediaKind::Video => f.write_str("video"),
            MediaKind::Text => f.write_str("text"),
            MediaKind::Application => f.write_str("application"),
            MediaKind::Message => f.write_str("message"),
            MediaKind::Other(s) => f.write_str(s),
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
    #[allow(clippy::too_many_arguments)]
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
            proto: String::new(),
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
        self.title = title.map(Into::into);
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
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{Media, MediaKind};
    use crate::sdp::attribute::Attribute;
    use crate::sdp::bandwidth::Bandwidth; // ensure your module is `bandwidth`
    use crate::sdp::connection::Connection;
    use crate::sdp::port_spec::PortSpec;
    use crate::sdp::sdpc::AddrType;

    // ---- MediaKind ----

    #[test]
    fn media_kind_display_and_from() {
        // Display
        assert_eq!(format!("{}", MediaKind::Audio), "audio");
        assert_eq!(format!("{}", MediaKind::Video), "video");
        assert_eq!(format!("{}", MediaKind::Text), "text");
        assert_eq!(format!("{}", MediaKind::Application), "application");
        assert_eq!(format!("{}", MediaKind::Message), "message");
        assert_eq!(format!("{}", MediaKind::Other("custom".into())), "custom");

        // From<&str>
        match MediaKind::from("audio") {
            MediaKind::Audio => {}
            _ => panic!("expected Audio"),
        }
        match MediaKind::from("video") {
            MediaKind::Video => {}
            _ => panic!("expected Video"),
        }
        match MediaKind::from("text") {
            MediaKind::Text => {}
            _ => panic!("expected Text"),
        }
        match MediaKind::from("application") {
            MediaKind::Application => {}
            _ => panic!("expected Application"),
        }
        match MediaKind::from("message") {
            MediaKind::Message => {}
            _ => panic!("expected Message"),
        }

        // Unknown / case-sensitive
        match MediaKind::from("Audio") {
            MediaKind::Other(s) => assert_eq!(s, "Audio"),
            _ => panic!("expected Other(\"Audio\")"),
        }
        match MediaKind::from("weird-kind") {
            MediaKind::Other(s) => assert_eq!(s, "weird-kind"),
            _ => panic!("expected Other(\"weird-kind\")"),
        }
    }

    // ---- Media ----

    #[test]
    fn new_sets_all_fields() {
        let m = Media::new(
            MediaKind::Audio,
            PortSpec::new(5004, None),
            "UDP/TLS/RTP/SAVPF",
            vec!["111".to_string(), "0".to_string()],
            Some("main audio".to_string()),
            Some(Connection::new("IN", AddrType::IP4, "203.0.113.1")),
            vec![Bandwidth::new("AS", 512), Bandwidth::new("TIAS", 64000)],
            vec![
                Attribute::new("rtcp-mux", None::<String>),
                Attribute::new("rtpmap", Some("111 opus/48000/2".to_string())),
            ],
            vec!["x-extra: foo".to_string(), "y-unknown: bar".to_string()],
        );

        // kind
        match m.kind() {
            MediaKind::Audio => {}
            _ => panic!("expected Audio"),
        }

        // port
        assert_eq!(m.port().base(), 5004);
        assert_eq!(m.port().num(), None);

        // proto
        assert_eq!(m.proto(), "UDP/TLS/RTP/SAVPF");

        // fmts
        assert_eq!(m.fmts().len(), 2);
        assert_eq!(m.fmts()[0], "111");
        assert_eq!(m.fmts()[1], "0");

        // title
        assert_eq!(m.title(), Some("main audio"));

        // connection
        let c = m.connection().expect("connection");
        assert_eq!(c.net_type(), "IN");
        assert!(matches!(c.addr_type(), &AddrType::IP4));
        assert_eq!(c.connection_address(), "203.0.113.1");

        // bandwidth
        assert_eq!(m.bandwidth().len(), 2);
        assert_eq!(m.bandwidth()[0].bwtype(), "AS");
        assert_eq!(m.bandwidth()[0].bandwidth(), 512);
        assert_eq!(m.bandwidth()[1].bwtype(), "TIAS");
        assert_eq!(m.bandwidth()[1].bandwidth(), 64000);

        // attrs
        assert_eq!(m.attrs().len(), 2);
        assert_eq!(m.attrs()[0].key(), "rtcp-mux");
        assert_eq!(m.attrs()[0].value(), None);
        assert_eq!(m.attrs()[1].key(), "rtpmap");
        assert_eq!(m.attrs()[1].value(), Some("111 opus/48000/2"));

        // extra
        assert_eq!(m.extra_lines().len(), 2);
        assert_eq!(m.extra_lines()[0], "x-extra: foo");
        assert_eq!(m.extra_lines()[1], "y-unknown: bar");
    }

    #[test]
    fn new_blank_defaults() {
        let m = Media::new_blank();
        match m.kind() {
            MediaKind::Audio => {}
            _ => panic!("new_blank kind should be Audio"),
        }
        assert_eq!(m.port().base(), 0);
        assert_eq!(m.port().num(), None);
        assert_eq!(m.proto(), "");
        assert!(m.fmts().is_empty());
        assert!(m.title().is_none());
        assert!(m.connection().is_none());
        assert!(m.bandwidth().is_empty());
        assert!(m.attrs().is_empty());
        assert!(m.extra_lines().is_empty());
    }

    #[test]
    fn setters_and_adders_update_fields() {
        let mut m = Media::new_blank();

        // kind
        m.set_kind(MediaKind::Video);
        match m.kind() {
            MediaKind::Video => {}
            _ => panic!("expected Video"),
        }

        // port / proto
        m.set_port(PortSpec::new(6000, Some(2)));
        m.set_proto("RTP/AVP");
        assert_eq!(m.port().base(), 6000);
        assert_eq!(m.port().num(), Some(2));
        assert_eq!(m.proto(), "RTP/AVP");

        // fmts
        m.set_fmts(vec!["96".into()]);
        m.add_fmt("97");
        assert_eq!(m.fmts(), &vec!["96".to_string(), "97".to_string()]);

        // title (Option<Into<String>> works with &str and String)
        m.set_title(Some("video track"));
        assert_eq!(m.title(), Some("video track"));
        m.set_title(None::<String>);
        assert_eq!(m.title(), None);
        m.set_title(Some(String::new()));
        assert_eq!(m.title(), Some(""));

        // connection
        m.set_connection(Some(Connection::new("IN", AddrType::IP6, "ff15::efc0:1")));
        let c = m.connection().unwrap();
        assert!(matches!(c.addr_type(), &AddrType::IP6));
        assert_eq!(c.connection_address(), "ff15::efc0:1");

        // bandwidth
        m.set_bandwidth(vec![Bandwidth::new("AS", 1024)]);
        m.add_bandwidth(Bandwidth::new("CT", 2048));
        assert_eq!(m.bandwidth().len(), 2);
        assert_eq!(m.bandwidth()[0].bwtype(), "AS");
        assert_eq!(m.bandwidth()[1].bwtype(), "CT");

        // attrs
        m.set_attrs(vec![Attribute::new("sendrecv", None::<String>)]);
        m.add_attr(Attribute::new("rtcp-fb", Some("nack".to_string())));
        assert_eq!(m.attrs().len(), 2);
        assert_eq!(m.attrs()[0].key(), "sendrecv");
        assert_eq!(m.attrs()[1].key(), "rtcp-fb");
        assert_eq!(m.attrs()[1].value(), Some("nack"));

        // extra lines
        m.set_extra_lines(vec!["x-foo: 1".into()]);
        m.add_extra_line("y-bar: 2");
        assert_eq!(
            m.extra_lines(),
            &vec!["x-foo: 1".to_string(), "y-bar: 2".to_string()]
        );
    }

    #[test]
    fn accepts_empty_and_whitespace_fields() {
        let mut m = Media::new_blank();

        // Empty proto and fmt are stored as-is
        m.set_proto("");
        m.add_fmt("");
        assert_eq!(m.proto(), "");
        assert_eq!(m.fmts(), &vec![String::new()]);

        // Empty title vs None
        m.set_title(Some(""));
        assert_eq!(m.title(), Some(""));
        m.set_title(None::<String>);
        assert_eq!(m.title(), None);

        // Extra lines can be empty/whitespace
        m.add_extra_line("");
        m.add_extra_line("  ");
        assert_eq!(m.extra_lines(), &vec![String::new(), "  ".to_string()]);
    }

    #[test]
    fn large_collections_are_supported() {
        let mut m = Media::new_blank();

        for i in 0..2_000u32 {
            m.add_fmt(i.to_string());
            m.add_attr(Attribute::new(format!("k{i}"), Some(format!("v{i}"))));
            m.add_bandwidth(Bandwidth::new("AS", u64::from(i)));
            m.add_extra_line(format!("x-{i}"));
        }

        assert_eq!(m.fmts().len(), 2_000);
        assert_eq!(m.attrs().len(), 2_000);
        assert_eq!(m.bandwidth().len(), 2_000);
        assert_eq!(m.extra_lines().len(), 2_000);

        // spot-check last elements (order preserved)
        assert_eq!(m.fmts().last().unwrap(), "1999");
        assert_eq!(m.attrs().last().unwrap().key(), "k1999");
        assert_eq!(m.bandwidth().last().unwrap().bandwidth(), 1999);
        assert_eq!(m.extra_lines().last().unwrap(), "x-1999");
    }
}
