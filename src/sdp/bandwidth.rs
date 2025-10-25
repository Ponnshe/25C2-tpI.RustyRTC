use crate::sdp::sdp_error::SdpError;
use std::{fmt, str::FromStr};
/// Represents the `b=` line of an SDP (Session Description Protocol).
///
/// Indicates the bandwidth of the session or of a specific media stream.
/// - `bwtype`: bandwidth type (for example `"AS"` for Application-Specific).
/// - `bandwidth`: bandwidth value in kbps.
#[derive(Debug, Clone)]
pub struct Bandwidth {
    bwtype: String,
    bandwidth: u64,
}

impl Bandwidth {
    /// Full constructor.
    ///
    /// # Parameters
    /// - `bwtype`: bandwidth type (`"AS"`, `"CT"`, etc.).
    /// - `bandwidth`: numeric value in kbps.
    ///
    /// # Example
    /// ```rust, ignore
    /// let b = Bandwidth::new("AS", 512);
    /// ```
    pub fn new(bwtype: impl Into<String>, bandwidth: u64) -> Self {
        Self {
            bwtype: bwtype.into(),
            bandwidth,
        }
    }

    /// Blank or default constructor.
    ///
    /// Default values:
    /// - `bwtype` = `"AS"`
    /// - `bandwidth` = `0`
    pub fn new_blank() -> Self {
        Self {
            bwtype: "AS".to_string(),
            bandwidth: 0,
        }
    }

    // --- GETTERS ---
    /// Returns the bandwidth type.
    pub fn bwtype(&self) -> &str {
        &self.bwtype
    }

    /// Returns the bandwidth value in kbps.
    pub const fn bandwidth(&self) -> u64 {
        self.bandwidth
    }

    // --- SETTERS ---
    /// Sets the bandwidth type.
    pub fn set_bwtype(&mut self, bwtype: impl Into<String>) {
        self.bwtype = bwtype.into();
    }

    /// Sets the bandwidth value in kbps.
    pub const fn set_bandwidth(&mut self, bandwidth: u64) {
        self.bandwidth = bandwidth;
    }
}

impl FromStr for Bandwidth {
    type Err = SdpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (typ, val) = s.split_once(':').ok_or(SdpError::Invalid("b="))?;
        Ok(Self::new(typ.to_owned(), val.parse::<u64>()?))
    }
}

impl fmt::Display for Bandwidth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.bwtype(), self.bandwidth())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::Bandwidth;

    #[test]
    fn new_sets_fields_correctly() {
        let b = Bandwidth::new("AS", 512);
        assert_eq!(b.bwtype(), "AS");
        assert_eq!(b.bandwidth(), 512);

        // Also ensure Into<String> works with String input
        let b2 = Bandwidth::new(String::from("CT"), 128);
        assert_eq!(b2.bwtype(), "CT");
        assert_eq!(b2.bandwidth(), 128);
    }

    #[test]
    fn new_blank_defaults() {
        let b = Bandwidth::new_blank();
        assert_eq!(b.bwtype(), "AS");
        assert_eq!(b.bandwidth(), 0);
    }

    #[test]
    fn setters_update_fields() {
        let mut b = Bandwidth::new_blank();

        b.set_bwtype("TIAS");
        b.set_bandwidth(2048);

        assert_eq!(b.bwtype(), "TIAS");
        assert_eq!(b.bandwidth(), 2048);

        // Update again to ensure values actually change
        b.set_bwtype("CT");
        b.set_bandwidth(1);
        assert_eq!(b.bwtype(), "CT");
        assert_eq!(b.bandwidth(), 1);
    }

    #[test]
    fn allows_empty_and_whitespace_bwtype() {
        let mut b = Bandwidth::new_blank();

        // Empty string should be stored as-is (validation happens elsewhere)
        b.set_bwtype("");
        assert_eq!(b.bwtype(), "");

        // Whitespace-only also stored as-is
        b.set_bwtype("  ");
        assert_eq!(b.bwtype(), "  ");

        // Weird/custom types are accepted
        b.set_bwtype("X-foo_1");
        assert_eq!(b.bwtype(), "X-foo_1");
    }

    #[test]
    fn handles_extreme_bandwidth_values() {
        let mut b = Bandwidth::new_blank();

        // Zero is valid
        b.set_bandwidth(0);
        assert_eq!(b.bandwidth(), 0);

        // Max u64 is accepted (no internal validation/overflow in the struct)
        b.set_bandwidth(u64::MAX);
        assert_eq!(b.bandwidth(), u64::MAX);
    }

    #[test]
    fn many_updates_do_not_panic_and_last_write_wins() {
        let mut b = Bandwidth::new_blank();

        // Simulate a bunch of updates (e.g., parser overwriting fields)
        for i in 0..10_000u64 {
            b.set_bandwidth(i);
        }
        assert_eq!(b.bandwidth(), 9_999);

        // Change bwtype repeatedly as well
        for i in 0..100 {
            b.set_bwtype(format!("AS{i}"));
        }
        assert!(b.bwtype().starts_with("AS"));
        assert!(b.bwtype().len() >= 3);
    }
}
