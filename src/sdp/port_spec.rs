use crate::sdp::sdp_error::SdpError;
use std::{fmt, str::FromStr};
/// Represents a port specifier (`m=`) in SDP.
///
/// Includes the base port and an optional number for hierarchical encoding,
/// although it is rarely used in WebRTC.
#[derive(Debug, Clone)]
pub struct PortSpec {
    base: u16,        // base port
    num: Option<u16>, // optional number of ports
}

impl PortSpec {
    /// Full constructor.
    ///
    /// # Parameters
    /// - `base`: base port
    /// - `num`: optional number for hierarchical encoding
    pub const fn new(base: u16, num: Option<u16>) -> Self {
        Self { base, num }
    }

    /// Default constructor.
    ///
    /// Default values:
    /// - `base` = 0
    /// - `num` = None
    pub const fn new_blank() -> Self {
        Self { base: 0, num: None }
    }

    // --- GETTERS ---
    /// Returns the base port.
    pub const fn base(&self) -> u16 {
        self.base
    }

    /// Returns the optional number.
    pub const fn num(&self) -> Option<u16> {
        self.num
    }

    // --- SETTERS ---
    /// Sets the base port.
    pub const fn set_base(&mut self, base: u16) {
        self.base = base;
    }

    /// Sets the optional number.
    pub const fn set_num(&mut self, num: Option<u16>) {
        self.num = num;
    }
}

impl FromStr for PortSpec {
    type Err = SdpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((a, b)) = s.split_once('/') {
            Ok(Self::new(a.parse::<u16>()?, Some(b.parse::<u16>()?)))
        } else {
            Ok(Self::new(s.parse::<u16>()?, None))
        }
    }
}

impl fmt::Display for PortSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.num() {
            Some(n) => write!(f, "{}/{}", self.base(), n),
            None => write!(f, "{}", self.base()),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::PortSpec;

    #[test]
    fn new_sets_fields_correctly() {
        let p = PortSpec::new(5004, Some(2));
        assert_eq!(p.base(), 5004);
        assert_eq!(p.num(), Some(2));
        assert_eq!(format!("{p}"), "5004/2");

        let p2 = PortSpec::new(3478, None);
        assert_eq!(p2.base(), 3478);
        assert_eq!(p2.num(), None);
        assert_eq!(format!("{p2}"), "3478");
    }

    #[test]
    fn new_blank_defaults() {
        let p = PortSpec::new_blank();
        assert_eq!(p.base(), 0);
        assert_eq!(p.num(), None);
        assert_eq!(format!("{p}"), "0");
    }

    #[test]
    fn setters_update_fields() {
        let mut p = PortSpec::new_blank();

        p.set_base(80);
        p.set_num(Some(4));
        assert_eq!(p.base(), 80);
        assert_eq!(p.num(), Some(4));
        assert_eq!(format!("{p}"), "80/4");

        // switch to None
        p.set_num(None);
        assert_eq!(p.num(), None);
        assert_eq!(format!("{p}"), "80");
    }

    #[test]
    fn display_formats_edge_cases() {
        // base = 0
        let mut p = PortSpec::new(0, None);
        assert_eq!(format!("{p}"), "0");

        // base = u16::MAX
        p.set_base(u16::MAX);
        assert_eq!(format!("{p}"), format!("{}", u16::MAX));

        // num = 0 should be preserved verbatim
        p.set_num(Some(0));
        assert_eq!(format!("{p}"), format!("{}/0", u16::MAX));

        // num = u16::MAX
        p.set_num(Some(u16::MAX));
        assert_eq!(format!("{p}"), format!("{}/{}", u16::MAX, u16::MAX));
    }

    #[test]
    fn many_updates_last_write_wins() {
        let mut p = PortSpec::new_blank();

        for i in 0..10_000u16 {
            p.set_base(i);
            // alternate between None and Some(i)
            if i % 2 == 0 {
                p.set_num(None);
            } else {
                p.set_num(Some(i));
            }
        }

        // After loop: i == 9999 (odd), so num = Some(9999)
        assert_eq!(p.base(), 9_999);
        assert_eq!(p.num(), Some(9_999));
        assert_eq!(format!("{p}"), "9999/9999");
    }
}
