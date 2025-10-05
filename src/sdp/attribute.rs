/// Representa un atributo `a=` en SDP.
///
/// Un atributo consiste en una clave y un valor opcional.  
/// Ejemplos comunes: `"rtpmap"`, `"fmtp"`, `"rtcp-mux"`.
#[derive(Debug)]
pub struct Attribute {
    key: String,
    value: Option<String>,
}

impl Attribute {
    /// Constructor completo.
    ///
    /// # Parámetros
    /// - `key`: clave del atributo, por ejemplo `"rtpmap"`.
    /// - `value`: valor opcional asociado al atributo.
    pub fn new<K: Into<String>, V: Into<Option<String>>>(key: K, value: V) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Constructor por defecto.
    ///
    /// Valores por defecto:
    /// - `key` = `""`
    /// - `value` = `None`
    pub fn new_blank() -> Self {
        Self {
            key: "".to_string(),
            value: None,
        }
    }

    // --- GETTERS ---
    /// Retorna una referencia a la clave del atributo.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Retorna una referencia al valor opcional del atributo.
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    // --- SETTERS ---
    /// Establece la clave del atributo.
    pub fn set_key<K: Into<String>>(&mut self, key: K) {
        self.key = key.into();
    }

    /// Establece el valor opcional del atributo.
    pub fn set_value<V: Into<Option<String>>>(&mut self, value: V) {
        self.value = value.into();
    }
}
