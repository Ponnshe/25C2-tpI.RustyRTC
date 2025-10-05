use std::fmt;

/// Representa un especificador de puerto (`m=`) en SDP.
///
/// Incluye el puerto base y un número opcional para codificación jerárquica,
/// aunque raramente se usa en WebRTC.
#[derive(Debug, Clone, Copy)]
pub struct PortSpec {
    base: u16,        // puerto base
    num: Option<u16>, // número opcional de puertos
}

impl PortSpec {
    /// Constructor completo.
    ///
    /// # Parámetros
    /// - `base`: puerto base
    /// - `num`: número opcional para codificación jerárquica
    pub fn new(base: u16, num: Option<u16>) -> Self {
        Self { base, num }
    }

    /// Constructor por defecto.
    ///
    /// Valores por defecto:
    /// - `base` = 0
    /// - `num` = None
    pub fn new_blank() -> Self {
        Self { base: 0, num: None }
    }

    // --- GETTERS ---
    /// Retorna el puerto base.
    pub fn base(&self) -> u16 {
        self.base
    }

    /// Retorna el número opcional.
    pub fn num(&self) -> Option<u16> {
        self.num
    }

    // --- SETTERS ---
    /// Establece el puerto base.
    pub fn set_base(&mut self, base: u16) {
        self.base = base;
    }

    /// Establece el número opcional.
    pub fn set_num(&mut self, num: Option<u16>) {
        self.num = num;
    }
}

impl fmt::Display for PortSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.num {
            Some(n) => write!(f, "{}/{}", self.base, n),
            None => write!(f, "{}", self.base),
        }
    }
}
