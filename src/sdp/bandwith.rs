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
