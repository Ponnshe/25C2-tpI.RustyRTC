use crate::sdp::addr_type::AddrType;
use crate::sdp::sdp_error::SdpError;
use std::{fmt, str::FromStr};

/// Represents the connection information of an SDP session.
///
/// This structure corresponds to the `c=` line in SDP (Session Description Protocol),
/// indicating network type, address type, and the unicast or multicast connection address.
#[derive(Debug, Clone)]
pub struct Connection {
    /// Network type, usually `"IN"` (Internet)
    net_type: String,
    /// Address type: IPv4 or IPv6
    addr_type: AddrType,
    /// Connection address, e.g. `"203.0.113.1"` or multicast addresses with `/ttl`
    conn_address: String,
}

impl Connection {
    /// Creates a new connection with specified values.
    ///
    /// # Parameters
    /// - `net_type`: network type, usually `"IN"`.
    /// - `addr_type`: address type (`AddrType::IP4` or `AddrType::IP6`).
    /// - `connection_address`: connection address.
    ///
    /// # Example
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
            conn_address: connection_address.into(),
        }
    }

    /// Creates a connection with default values.
    ///
    /// - `net_type` = `"IN"`
    /// - `addr_type` = `IP4`
    /// - `connection_address` = `"127.0.0.1"`
    ///
    /// Useful as a placeholder for tests or quick initialization.
    #[must_use]
    pub fn new_blank() -> Self {
        Self {
            net_type: "IN".to_string(),
            addr_type: AddrType::IP4,
            conn_address: "127.0.0.1".to_string(),
        }
    }

    // --- GETTERS ---

    /// Returns a reference to the network type.
    #[must_use]
    pub fn net_type(&self) -> &str {
        &self.net_type
    }

    /// Returns a reference to the address type.
    #[must_use]
    pub const fn addr_type(&self) -> &AddrType {
        &self.addr_type
    }

    /// Returns a reference to the connection address.
    #[must_use]
    pub fn connection_address(&self) -> &str {
        &self.conn_address
    }

    // --- SETTERS ---

    /// Sets the network type.
    pub fn set_net_type(&mut self, net_type: String) {
        self.net_type = net_type;
    }

    /// Sets the address type.
    pub const fn set_addr_type(&mut self, addr_type: AddrType) {
        self.addr_type = addr_type;
    }

    /// Sets the connection address.
    pub fn set_connection_address(&mut self, address: String) {
        self.conn_address = address;
    }
}

impl FromStr for Connection {
    type Err = SdpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // nettype addrtype address
        let parts: Vec<_> = s.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(SdpError::Invalid("c="));
        }
        Ok(Self::new(
            parts[0].to_owned(),
            parts[1].parse().map_err(|()| SdpError::AddrType)?,
            parts[2].to_owned(),
        ))
    }
}

impl fmt::Display for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.net_type(),
            self.addr_type(),
            self.connection_address()
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::Connection;
    use crate::sdp::addr_type::AddrType;

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
