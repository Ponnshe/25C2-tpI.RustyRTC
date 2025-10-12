use crate::ice::type_ice::ice_agent::IceAgent;
use crate::sdp::sdpc::Sdp;

/// Gestiona el proceso completo de una conexión P2P, coordinando ICE y SDP.
pub struct ConnectionManager {
    ice_agent: IceAgent,
    // Otros campos necesarios para gestionar la conexión.
}

impl ConnectionManager {
    /// Crea un nuevo gestor de conexiones.
    pub fn new() -> Self {
        todo!()
    }

    /// Inicia el proceso de conexion generando una oferta SDP.
    /// Internamente, recolecta candidatos ICE y los añade a la oferta.
    pub fn create_offer(&mut self) -> Result<Sdp, String> {
        todo!()
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
}
