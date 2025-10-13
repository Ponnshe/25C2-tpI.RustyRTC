use super::{connection_error::ConnectionError, ice_candidate_to_sdp::ICEToSDP};
use crate::ice::gathering_service;
use crate::ice::type_ice::ice_agent::{IceAgent, IceRole};
use crate::sdp::addr_type::AddrType as SDPAddrType;
use crate::sdp::attribute::Attribute as SDPAttribute;
use crate::sdp::connection::Connection as SDPConnection;
use crate::sdp::media::Media as SDPMedia;
use crate::sdp::media::MediaKind as SDPMediaKind;
use crate::sdp::origin::Origin as SDPOrigin;
use crate::sdp::port_spec::PortSpec as SDPPortSpec;
use crate::sdp::sdpc::Sdp;
use crate::sdp::time_desc::TimeDesc as SDPTimeDesc;

const DEFAULT_PORT: u16 = 9;
const DEAFULT_PROTO: &str = "UDP/TLS/RTP/SAVPF";
const DEFAULT_FMT: &str = "99";
const DEFAULT_NET_TYPE: &str = "IN";
const DEFAULT_ADDR_TYPE: SDPAddrType = SDPAddrType::IP4;
const DEFAULT_CONN_ADDR: &str = "0.0.0.0";
const DEFAULT_CODEC: &str = "VP8 90000";
const DEAULT_MEDIA_KIND: SDPMediaKind = SDPMediaKind::Application;

/// Gestiona el proceso completo de una conexión P2P, coordinando ICE y SDP.
pub struct ConnectionManager {
    ice_agent: IceAgent,
    // Otros campos necesarios para gestionar la conexión.
}

impl ConnectionManager {
    pub fn new() -> Self {
        let ice_agent = IceAgent::new(IceRole::Controlling);
        Self {
            ice_agent: ice_agent,
        }
    }

    /// Crea un nuevo gestor de conexiones.

    /// Inicia el proceso de conexion generando una oferta SDP.
    /// Internamente, recolecta candidatos ICE y los añade a la oferta.
    pub fn create_offer(&mut self) -> Result<Sdp, ConnectionError> {
        let version: u8 = 0;
        let origin = SDPOrigin::new_blank();
        let session_name = "demo_session".to_owned();
        let session_info = None;
        let uri = None;
        let emails = Vec::new();
        let phones = Vec::new();
        let connection = None;
        let bandwidth = Vec::new();
        let times: Vec<SDPTimeDesc> = vec![SDPTimeDesc::new_blank()];
        let attrs = Vec::new();
        let media: Vec<SDPMedia> = vec![mocked_media_spec_to_media_description()?];
        let extra_lines = Vec::new();
        let sdp = Sdp::new(
            version,
            origin,
            session_name,
            session_info,
            uri,
            emails,
            phones,
            connection,
            bandwidth,
            times,
            attrs,
            media,
            extra_lines,
        );
        Ok(sdp)
    }

    /// Recibe una oferta SDP de un par remoto y genera una respuesta.
    /// Parsea los candidatos remotos, recolecta los propios y crea la respuesta SDP.
    pub fn receive_offer_and_create_answer(&mut self, offer: Sdp) -> Result<Sdp, String> {
        todo!()
    }

    /// (Para el oferente) Recibe la respuesta SDP del par remoto.
    /// Parsea los candidatos remotos de la respuesta para completar la negociacion.
    pub fn receive_answer(&mut self, answer: Sdp) -> Result<(), String> {
        todo!()
    }

    /// Ejecuta las verificaciones de conectividad (envía y recibe STUN).
    /// Es `async` porque implica esperar I/O de red.
    pub async fn start_connectivity_checks(&mut self) {
        todo!()
    }

    pub fn set_ice_agent(&mut self, ice_agent: IceAgent) {
        self.ice_agent = ice_agent;
    }
}

const DEFAULT_PORT: u16 = 9;
const DEAFULT_PROTO: &str = "UDP/TLS/RTP/SAVPF";
const DEFAULT_FMT: &str = "99";
const DEFAULT_NET_TYPE: &str = "IN";
const DEFAULT_ADDR_TYPE: SDPAddrType = SDPAddrType::IP4;
const DEFAULT_CONN_ADDR: &str = "0.0.0.0";
const DEFAULT_CODEC: &str = "VP8 90000";
const DEAULT_MEDIA_KIND: SDPMediaKind = SDPMediaKind::Application;

fn get_candidates_as_attributes() -> Vec<SDPAttribute> {
    gathering_service::gather_host_candidates()
        .into_iter()
        .map(|c| {
            let ice_cand_to_sdp = ICEToSDP::new(c);
            SDPAttribute::new("candidate", ice_cand_to_sdp.to_string())
        })
        .collect::<Vec<SDPAttribute>>()
}
