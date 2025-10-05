use crate::sdp::sdpc::AddrType;

/// Representa la información de conexión de una sesión SDP.
///
/// Esta estructura corresponde a la línea `c=` en SDP (Session Description Protocol),
/// indicando tipo de red, tipo de dirección y la dirección de conexión unicast o multicast.
#[derive(Debug)]
pub struct Connection {
    /// Tipo de red, usualmente `"IN"` (Internet)
    net_type: String,
    /// Tipo de dirección: IPv4 o IPv6
    addr_type: AddrType,
    /// Dirección de conexión, por ejemplo `"203.0.113.1"` o direcciones multicast con `/ttl`
    connection_address: String,
}

impl Connection {
    /// Crea una nueva conexión con valores especificados.
    ///
    /// # Parámetros
    /// - `net_type`: tipo de red, usualmente `"IN"`.
    /// - `addr_type`: tipo de dirección (`AddrType::IP4` o `AddrType::IP6`).
    /// - `connection_address`: dirección de conexión.
    ///
    /// # Ejemplo
    /// ```rust
    /// let conn = Connection::new("IN", AddrType::IP4, "203.0.113.1");
    /// ```
    pub fn new(
        net_type: impl Into<String>,
        addr_type: AddrType,
        connection_address: impl Into<String>,
    ) -> Self {
        Self {
            net_type: net_type.into(),
            addr_type,
            connection_address: connection_address.into(),
        }
    }

    /// Crea una conexión con valores por defecto.
    ///
    /// - `net_type` = `"IN"`
    /// - `addr_type` = `IP4`
    /// - `connection_address` = `"127.0.0.1"`
    ///
    /// Útil como placeholder para pruebas o inicialización rápida.
    pub fn new_blank() -> Self {
        Self {
            net_type: "IN".to_string(),
            addr_type: AddrType::IP4,
            connection_address: "127.0.0.1".to_string(),
        }
    }

    // --- GETTERS ---

    /// Retorna una referencia al tipo de red.
    pub fn net_type(&self) -> &str {
        &self.net_type
    }

    /// Retorna una referencia al tipo de dirección.
    pub fn addr_type(&self) -> &AddrType {
        &self.addr_type
    }

    /// Retorna una referencia a la dirección de conexión.
    pub fn connection_address(&self) -> &str {
        &self.connection_address
    }

    // --- SETTERS ---

    /// Modifica el tipo de red.
    pub fn set_net_type(&mut self, net_type: String) {
        self.net_type = net_type;
    }

    /// Modifica el tipo de dirección.
    pub fn set_addr_type(&mut self, addr_type: AddrType) {
        self.addr_type = addr_type;
    }

    /// Modifica la dirección de conexión.
    pub fn set_connection_address(&mut self, address: String) {
        self.connection_address = address;
    }
}
