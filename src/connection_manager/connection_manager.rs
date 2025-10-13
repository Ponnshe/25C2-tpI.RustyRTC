use super::{connection_error::ConnectionError, ice_and_sdp::ICEAndSDP};
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
const DEFAULT_MEDIA_KIND: SDPMediaKind = SDPMediaKind::Video;

/// Gestiona el proceso completo de una conexión P2P, coordinando ICE y SDP.
pub struct ConnectionManager {
    pub ice_agent: IceAgent,
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
        let media: Vec<SDPMedia> = vec![get_mocked_media_description(self)?];
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
    pub fn receive_offer_and_create_answer(&mut self, offer: &str) -> Result<Sdp, ConnectionError> {
        let sdp_offer = Sdp::parse(offer).map_err(|e| ConnectionError::Sdp(e))?;

        // TODO: pasar esto a un modulo aparte que se encargue de manejar media y sus atributos
        for m in sdp_offer.media() {
            for a in m.attrs() {
                if a.key() == "candidate" {
                    let value = a.value().ok_or(ConnectionError::IceAgent)?;
                    let ice_and_sdp: ICEAndSDP =
                        value.parse().map_err(|_| ConnectionError::IceAgent)?;
                    self.ice_agent.add_remote_candidate(ice_and_sdp.candidate());
                }
            }
        }

        let sdp_answer = self.create_offer()?;
        Ok(sdp_answer)
    }

    /// (Para el oferente) Recibe la respuesta SDP del par remoto.
    /// Parsea los candidatos remotos de la respuesta para completar la negociacion.
    pub fn receive_answer(&mut self, answer: Sdp) -> Result<(), ConnectionError> {
        // TODO Mismo caso que arriba
        for m in answer.media() {
            for a in m.attrs() {
                if a.key() == "candidate" {
                    let value = a.value().ok_or(ConnectionError::IceAgent)?;
                    let ice_and_sdp: ICEAndSDP =
                        value.parse().map_err(|_| ConnectionError::IceAgent)?;
                    self.ice_agent.add_remote_candidate(ice_and_sdp.candidate());
                }
            }
        }

        Ok(())
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

fn get_mocked_media_description(
    conn_manager: &mut ConnectionManager,
) -> Result<SDPMedia, ConnectionError> {
    let mut media_desc = SDPMedia::new_blank();
    media_desc.set_kind(DEFAULT_MEDIA_KIND);
    let port_spec_sdp = SDPPortSpec::new(DEFAULT_PORT, None);
    media_desc.set_port(port_spec_sdp);
    media_desc.set_proto(DEAFULT_PROTO);
    let fmts = vec![DEFAULT_FMT.to_owned()];
    media_desc.set_fmts(fmts);
    let connection_sdp = SDPConnection::new(DEFAULT_NET_TYPE, DEFAULT_ADDR_TYPE, DEFAULT_CONN_ADDR);
    media_desc.set_connection(Some(connection_sdp));
    let mut attrs = get_local_candidates_as_attributes(conn_manager);
    let (ufrag, pwd) = conn_manager.ice_agent.local_credentials(); // or (mock_ufrag(), mock_pwd())
    attrs.push(SDPAttribute::new("ice-ufrag", ufrag));
    attrs.push(SDPAttribute::new("ice-pwd", pwd));
    attrs.push(SDPAttribute::new("rtpmap", Some("96 VP8/90000".to_owned())));
    attrs.push(SDPAttribute::new("rtcp-mux", Some("".to_owned())));
    media_desc.set_attrs(attrs);
    Ok(media_desc)
}

fn get_local_candidates_as_attributes(conn_manager: &mut ConnectionManager) -> Vec<SDPAttribute> {
    // TODO reemplazar el gathering por el de ICE agent
    gathering_service::gather_host_candidates()
        .into_iter()
        .map(|c| {
            let ice_cand_to_sdp = ICEAndSDP::new(c);
            let attr = SDPAttribute::new("candidate", ice_cand_to_sdp.to_string());
            conn_manager
                .ice_agent
                .add_local_candidate(ice_cand_to_sdp.candidate());
            attr
        })
        .collect::<Vec<SDPAttribute>>()
}
