use crate::sdp::sdp_error::SdpError;
use std::{fmt, str::FromStr};
/// Represents an `a=` attribute in SDP.
///
/// An attribute consists of a key and an optional value.
/// Common examples: `"rtpmap"`, `"fmtp"`, `"rtcp-mux"`.
#[derive(Debug)]
pub struct Attribute {
    key: String,
    value: Option<String>,
}

impl Attribute {
    /// Full constructor.
    ///
    /// # Parameters
    /// - `key`: attribute key, e.g., `"rtpmap"`.
    /// - `value`: optional value associated with the attribute.
    pub fn new<K: Into<String>, V: Into<Option<String>>>(key: K, value: V) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Default constructor.
    ///
    /// Default values:
    /// - `key` = `""`
    /// - `value` = `None`
    pub const fn new_blank() -> Self {
        Self {
            key: String::new(),
            value: None,
        }
    }

    // --- GETTERS ---
    /// Returns a reference to the attribute key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns a reference to the optional attribute value.
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    // --- SETTERS ---
    /// Sets the attribute key.
    pub fn set_key<K: Into<String>>(&mut self, key: K) {
        self.key = key.into();
    }

    /// Sets the optional attribute value.
    pub fn set_value<V: Into<Option<String>>>(&mut self, value: V) {
        self.value = value.into();
    }
}

impl FromStr for Attribute {
    type Err = SdpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (k, v) = if let Some((k, v)) = s.split_once(':') {
            (k.trim().to_owned(), Some(v.trim().to_owned()))
        } else {
            (s.trim().to_owned(), None)
        };
        Ok(Self::new(k, v))
    }
}

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(v) = self.value() {
            write!(f, "{}:{}", self.key(), v)
        } else {
            write!(f, "{}", self.key())
        }
    }
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::Attribute;

    #[test]
    fn new_with_some_string_sets_fields() {
        let a = Attribute::new("rtpmap", Some(String::from("111 opus/48000/2")));
        assert_eq!(a.key(), "rtpmap");
        assert_eq!(a.value(), Some("111 opus/48000/2"));
    }

    #[test]
    fn new_with_some_string_alt() {
        // Same idea as the previous test, using to_string()
        let a = Attribute::new("fmtp", Some("111 minptime=10;usedtx=1".to_string()));
        assert_eq!(a.key(), "fmtp");
        assert_eq!(a.value(), Some("111 minptime=10;usedtx=1"));
    }

    #[test]
    fn new_with_none_sets_value_none() {
        let a = Attribute::new("rtcp-mux", None::<String>);
        assert_eq!(a.key(), "rtcp-mux");
        assert_eq!(a.value(), None);
    }

    #[test]
    fn new_blank_defaults() {
        let a = Attribute::new_blank();
        assert_eq!(a.key(), "");
        assert_eq!(a.value(), None);
    }

    #[test]
    fn setters_update_key_and_value() {
        let mut a = Attribute::new_blank();

        a.set_key("ice-ufrag");
        a.set_value(Some("abcd".to_string()));
        assert_eq!(a.key(), "ice-ufrag");
        assert_eq!(a.value(), Some("abcd"));

        // Limpiar valor a None
        a.set_value(None::<String>);
        assert_eq!(a.value(), None);

        // Acepta valor vacío explícito (distinto de None)
        a.set_value(Some(String::new()));
        assert_eq!(a.value(), Some(""));

        // Cambiar clave nuevamente
        a.set_key(String::from("ice-pwd"));
        assert_eq!(a.key(), "ice-pwd");
    }

    #[test]
    fn allows_empty_whitespace_and_weird_keys() {
        let mut a = Attribute::new_blank();

        a.set_key("");
        assert_eq!(a.key(), "");

        a.set_key("  ");
        assert_eq!(a.key(), "  ");

        a.set_key("x-foo:bar;baz");
        assert_eq!(a.key(), "x-foo:bar;baz");
    }

    #[test]
    fn many_updates_last_write_wins() {
        let mut a = Attribute::new("k0", Some("v0".to_string()));

        for i in 1..10_000 {
            a.set_key(format!("k{i}"));
            a.set_value(Some(format!("v{i}")));
        }

        assert_eq!(a.key(), "k9999");
        assert_eq!(a.value(), Some("v9999"));

        // Y limpiar al final
        a.set_value(None::<String>);
        assert_eq!(a.value(), None);
    }
}
