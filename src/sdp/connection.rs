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
    /// ```rust, ignore
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
    pub const fn addr_type(&self) -> &AddrType {
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
    pub const fn set_addr_type(&mut self, addr_type: AddrType) {
        self.addr_type = addr_type;
    }

    /// Modifica la dirección de conexión.
    pub fn set_connection_address(&mut self, address: String) {
        self.connection_address = address;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::Connection;
    use crate::sdp::sdpc::AddrType;

    #[test]
    fn new_sets_fields_correctly_ipv4() {
        let c = Connection::new("IN", AddrType::IP4, "203.0.113.1");
        assert_eq!(c.net_type(), "IN");
        assert!(matches!(c.addr_type(), &AddrType::IP4));
        assert_eq!(c.connection_address(), "203.0.113.1");
    }

    #[test]
    fn new_sets_fields_correctly_ipv6() {
        let c = Connection::new(String::from("IN"), AddrType::IP6, "::1");
        assert_eq!(c.net_type(), "IN");
        assert!(matches!(c.addr_type(), &AddrType::IP6));
        assert_eq!(c.connection_address(), "::1");
    }

    #[test]
    fn new_blank_defaults() {
        let c = Connection::new_blank();
        assert_eq!(c.net_type(), "IN");
        assert!(matches!(c.addr_type(), &AddrType::IP4));
        assert_eq!(c.connection_address(), "127.0.0.1");
    }

    #[test]
    fn setters_update_fields() {
        let mut c = Connection::new_blank();

        c.set_net_type("IN".to_string());
        c.set_addr_type(AddrType::IP6);
        c.set_connection_address("ff02::1".to_string());

        assert_eq!(c.net_type(), "IN");
        assert!(matches!(c.addr_type(), &AddrType::IP6));
        assert_eq!(c.connection_address(), "ff02::1");

        // Actualizar de nuevo para verificar sobreescritura
        c.set_net_type("ATM".to_string());
        c.set_addr_type(AddrType::IP4);
        c.set_connection_address("224.2.1.1/127".to_string());

        assert_eq!(c.net_type(), "ATM");
        assert!(matches!(c.addr_type(), &AddrType::IP4));
        assert_eq!(c.connection_address(), "224.2.1.1/127");
    }

    #[test]
    fn accepts_empty_and_whitespace_net_type_and_address() {
        let mut c = Connection::new_blank();

        c.set_net_type(String::new());
        c.set_connection_address(String::new());
        assert_eq!(c.net_type(), "");
        assert_eq!(c.connection_address(), "");

        c.set_net_type("  ".to_string());
        c.set_connection_address("  ".to_string());
        assert_eq!(c.net_type(), "  ");
        assert_eq!(c.connection_address(), "  ");
    }

    #[test]
    fn multicast_and_ttl_syntax_is_stored_verbatim() {
        let mut c = Connection::new("IN", AddrType::IP4, "224.2.1.1/127");
        assert_eq!(c.connection_address(), "224.2.1.1/127");

        c.set_addr_type(AddrType::IP6);
        c.set_connection_address("ff15::efc0:1/64".to_string());
        assert!(matches!(c.addr_type(), &AddrType::IP6));
        assert_eq!(c.connection_address(), "ff15::efc0:1/64");
    }

    #[test]
    fn many_updates_last_write_wins() {
        let mut c = Connection::new_blank();

        for i in 0..5_000u32 {
            c.set_net_type(format!("NET{i}"));
            if i % 2 == 0 {
                c.set_addr_type(AddrType::IP4);
            } else {
                c.set_addr_type(AddrType::IP6);
            }
            c.set_connection_address(format!("10.0.0.{i}"));
        }

        assert!(c.net_type().starts_with("NET"));
        // 4_999 es impar, por lo tanto terminó en IP6
        assert!(matches!(c.addr_type(), &AddrType::IP6));
        assert_eq!(c.connection_address(), "10.0.0.4999");
    }
}
