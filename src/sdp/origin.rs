use std::time::{SystemTime, UNIX_EPOCH};

use crate::sdp::sdpc::AddrType;

fn ntp_seconds() -> u64 {
    // NTP epoch starts at 1900, UNIX_EPOCH starts at 1970
    const NTP_UNIX_DIFF: u64 = 2_208_988_800; // segundos entre 1900 y 1970
    let unix_now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    unix_now + NTP_UNIX_DIFF
}

#[derive(Debug)]
pub struct Origin {
    username: String,
    session_id: u64,
    session_version: u64,
    net_type: String,    // usually "IN"
    addr_type: AddrType, // IP4 or IP6
    unicast_address: String,
}

impl Origin {
    /// Constructor
    pub fn new (username: impl Into<String>, session_id: u64, session_version: u64, net_type: impl Into<String>, addr_type: AddrType, unicast_address: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            session_id,
            session_version,
            net_type: net_type.into(),
            addr_type,
            unicast_address: unicast_address.into(),
        }
    }

    pub fn new_blank() -> Self {
        let session_id = ntp_seconds();
        Self {
            username: "-".to_string(),
            session_id,
            session_version: session_id,
            net_type: "IN".to_string(),
            addr_type: AddrType::IP4,
            unicast_address:"".to_string(),
        }
    }

    // ---------------- Getters ----------------
    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    pub fn session_version(&self) -> u64 {
        self.session_version
    }

    pub fn net_type(&self) -> &str {
        &self.net_type
    }

    pub fn addr_type(&self) -> &AddrType {
        &self.addr_type
    }

    pub fn unicast_address(&self) -> &str {
        &self.unicast_address
    }

    // ---------------- Setters ----------------
    pub fn set_username<U: Into<String>>(&mut self, username: U) {
        self.username = username.into();
    }

    pub fn set_session_id(&mut self, session_id: u64) {
        self.session_id = session_id;
    }

    pub fn set_session_version(&mut self, session_version: u64) {
        self.session_version = session_version;
    }

    pub fn set_net_type<N: Into<String>>(&mut self, net_type: N) {
        self.net_type = net_type.into();
    }

    pub fn set_addr_type(&mut self, addr_type: AddrType) {
        self.addr_type = addr_type;
    }

    pub fn set_unicast_address<U: Into<String>>(&mut self, unicast_address: U) {
        self.unicast_address = unicast_address.into();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{AddrType, Origin, ntp_seconds};

    #[test]
    fn new_sets_fields_correctly() {
        let o = Origin::new(
            String::from("-"),
            42,
            7,
            String::from("IN"), // show that `N: Into<String>` accepts String
            AddrType::IP4,
            "127.0.0.1",
        );

        assert_eq!(o.username(), "-");
        assert_eq!(o.session_id(), 42);
        assert_eq!(o.session_version(), 7);
        assert_eq!(o.net_type(), "IN");
        assert!(matches!(*o.addr_type(), AddrType::IP4));
        assert_eq!(o.unicast_address(), "127.0.0.1");
    }

    #[test]
    fn new_blank_sets_sane_defaults() {
        // Bound the generated NTP time to avoid flakiness
        let before = ntp_seconds();
        let o = Origin::new_blank();
        let after = ntp_seconds();

        assert_eq!(o.username(), "-");
        assert_eq!(o.net_type(), "IN");
        assert!(matches!(*o.addr_type(), AddrType::IP4));
        assert_eq!(o.unicast_address(), "");

        // session_id should be "now" in NTP seconds and equal to session_version
        assert!(o.session_id() >= before && o.session_id() <= after);
        assert_eq!(o.session_version(), o.session_id());
    }

    #[test]
    fn setters_update_fields() {
        let mut o = Origin::new_blank();

        o.set_username("alice");
        o.set_session_id(100);
        o.set_session_version(101);
        o.set_net_type("IN"); // keep as IN, just exercising setter
        o.set_addr_type(AddrType::IP6);
        o.set_unicast_address("::1");

        assert_eq!(o.username(), "alice");
        assert_eq!(o.session_id(), 100);
        assert_eq!(o.session_version(), 101);
        assert_eq!(o.net_type(), "IN");
        assert!(matches!(*o.addr_type(), AddrType::IP6));
        assert_eq!(o.unicast_address(), "::1");
    }
}
