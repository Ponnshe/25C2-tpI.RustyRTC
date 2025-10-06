use crate::sdp::addr_type::AddrType;
use crate::sdp::sdp_error::SdpError; // adjust path if SdpError is elsewhere
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fmt, str::FromStr};

/// Computes the current NTP seconds (epoch 1900) from the `UNIX_EPOCH` (1970).
///
/// Used to generate default values for `session_id` and `session_version` in SDP.
fn ntp_seconds() -> u64 {
    const NTP_UNIX_DIFF: u64 = 2_208_988_800; // seconds between 1900 and 1970
    let unix_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|err| {
            eprintln!("Advertencia: reloj antes de UNIX_EPOCH: {err:?}");
            std::time::Duration::from_secs(0)
        })
        .as_secs();

    unix_now + NTP_UNIX_DIFF
}

/// Represents the `o=` line of an SDP (Session Description Protocol).
///
/// Contains the session origin information:
/// - `username`: name of the user who originated the session.
/// - `session_id`: unique session identifier (NTP seconds recommended for uniqueness).
/// - `session_version`: session version, usually equal to `session_id` initially.
/// - `net_type`: network type (usually `"IN"` for Internet).
/// - `addr_type`: address type (IPv4 or IPv6).
/// - `unicast_address`: origin unicast address (host IP).
#[derive(Debug)]
pub struct Origin {
    username: String,
    session_id: u64,
    session_version: u64,
    net_type: String,
    addr_type: AddrType,
    unicast_address: String,
}

impl Origin {
    /// Creates a new `Origin` instance with all specified values.
    ///
    /// # Parameters
    /// - `username`: name of the user initiating the session.
    /// - `session_id`: unique session identifier.
    /// - `session_version`: session version.
    /// - `net_type`: network type (e.g., `"IN"`).
    /// - `addr_type`: address type (`AddrType::IP4` or `AddrType::IP6`).
    /// - `unicast_address`: origin unicast address.
    ///
    /// # Example
    /// ```rust, ignore
    /// let origin = Origin::new("alice", 12345, 12345, "IN", AddrType::IP4, "192.168.1.1");
    /// ```
    pub fn new(
        username: impl Into<String>,
        session_id: u64,
        session_version: u64,
        net_type: impl Into<String>,
        addr_type: AddrType,
        unicast_address: impl Into<String>,
    ) -> Self {
        Self {
            username: username.into(),
            session_id,
            session_version,
            net_type: net_type.into(),
            addr_type,
            unicast_address: unicast_address.into(),
        }
    }

    /// Creates an `Origin` instance with default values.
    ///
    /// - `username` = `"-"` (placeholder)
    /// - `session_id` and `session_version` = current NTP seconds
    /// - `net_type` = `"IN"`
    /// - `addr_type` = `IP4`
    /// - `unicast_address` = `""` (empty)
    ///
    /// Useful to quickly initialize an SDP without specific values.
    pub fn new_blank() -> Self {
        let session_id = ntp_seconds();
        Self {
            username: "-".to_string(),
            session_id,
            session_version: session_id,
            net_type: "IN".to_string(),
            addr_type: AddrType::IP4,
            unicast_address: String::new(),
        }
    }

    // ---------------- Getters ----------------

    /// Returns the origin username.
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Returns the session identifier.
    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    /// Returns the session version.
    pub const fn session_version(&self) -> u64 {
        self.session_version
    }

    /// Returns the network type (generally `"IN"`).
    pub fn net_type(&self) -> &str {
        &self.net_type
    }

    /// Returns the address type (IPv4 or IPv6).
    pub const fn addr_type(&self) -> &AddrType {
        &self.addr_type
    }

    /// Returns the origin unicast address.
    pub fn unicast_address(&self) -> &str {
        &self.unicast_address
    }

    // ---------------- Setters ----------------

    /// Sets the origin username.
    pub fn set_username<U: Into<String>>(&mut self, username: U) {
        self.username = username.into();
    }

    /// Sets the session identifier.
    pub const fn set_session_id(&mut self, session_id: u64) {
        self.session_id = session_id;
    }

    /// Sets the session version.
    pub const fn set_session_version(&mut self, session_version: u64) {
        self.session_version = session_version;
    }

    /// Sets the network type.
    pub fn set_net_type<N: Into<String>>(&mut self, net_type: N) {
        self.net_type = net_type.into();
    }

    /// Sets the address type (IPv4 or IPv6).
    pub const fn set_addr_type(&mut self, addr_type: AddrType) {
        self.addr_type = addr_type;
    }

    /// Sets the origin unicast address.
    pub fn set_unicast_address<U: Into<String>>(&mut self, unicast_address: U) {
        self.unicast_address = unicast_address.into();
    }
}

impl FromStr for Origin {
    type Err = SdpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // username sess-id sess-version nettype addrtype unicast
        let parts: Vec<_> = s.split_whitespace().collect();
        if parts.len() != 6 {
            return Err(SdpError::Invalid("o="));
        }
        Ok(Self::new(
            parts[0].to_owned(),
            parts[1].parse::<u64>()?,
            parts[2].parse::<u64>()?,
            parts[3].to_owned(),
            parts[4].parse().map_err(|()| SdpError::AddrType)?,
            parts[5].to_owned(),
        ))
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {} {} {}",
            self.username(),
            self.session_id(),
            self.session_version(),
            self.net_type(),
            self.addr_type(),
            self.unicast_address()
        )
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
