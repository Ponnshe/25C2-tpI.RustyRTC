/// Representa la línea `b=` de un SDP (Session Description Protocol).
///
/// Indica el ancho de banda de la sesión o de un media stream específico.
/// - `bwtype`: tipo de ancho de banda (por ejemplo `"AS"` para Application-Specific).
/// - `bandwidth`: valor del ancho de banda en kbps.
#[derive(Debug)]
pub struct Bandwidth {
    bwtype: String,
    bandwidth: u64,
}

impl Bandwidth {
    /// Constructor completo.
    ///
    /// # Parámetros
    /// - `bwtype`: tipo de ancho de banda (`"AS"`, `"CT"`, etc.).
    /// - `bandwidth`: valor numérico en kbps.
    ///
    /// # Ejemplo
    /// ```rust
    /// let b = Bandwidth::new("AS", 512);
    /// ```
    pub fn new(bwtype: impl Into<String>, bandwidth: u64) -> Self {
        Self {
            bwtype: bwtype.into(),
            bandwidth,
        }
    }

    /// Constructor "vacío" o por defecto.
    ///
    /// Valores por defecto:
    /// - `bwtype` = `"AS"`
    /// - `bandwidth` = `0`
    pub fn new_blank() -> Self {
        Self {
            bwtype: "AS".to_string(),
            bandwidth: 0,
        }
    }

    // --- GETTERS ---
    /// Retorna el tipo de ancho de banda.
    pub fn bwtype(&self) -> &str {
        &self.bwtype
    }

    /// Retorna el valor del ancho de banda en kbps.
    pub fn bandwidth(&self) -> u64 {
        self.bandwidth
    }

    // --- SETTERS ---
    /// Establece el tipo de ancho de banda.
    pub fn set_bwtype(&mut self, bwtype: impl Into<String>) {
        self.bwtype = bwtype.into();
    }

    /// Establece el valor del ancho de banda en kbps.
    pub fn set_bandwidth(&mut self, bandwidth: u64) {
        self.bandwidth = bandwidth;
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
