/// Representa un bloque de tiempo (`t=`) en SDP (Session Description Protocol),
/// incluyendo repeticiones (`r=`) y zonas horarias (`z=`).
#[derive(Debug)]
pub struct TimeDesc {
    start: u64,           // tiempo de inicio en segundos NTP, usualmente 0
    stop: u64,            // tiempo de fin en segundos NTP, usualmente 0
    repeats: Vec<String>, // líneas r= crudas
    zone: Option<String>, // línea z= cruda (zona horaria)
}

impl TimeDesc {
    /// Constructor completo.
    ///
    /// # Parámetros
    /// - `start`: tiempo de inicio en segundos NTP
    /// - `stop`: tiempo de fin en segundos NTP
    /// - `repeats`: vectores de líneas r= crudas
    /// - `zone`: opcional, línea z= cruda
    pub fn new(
        start: u64,
        stop: u64,
        repeats: Vec<String>,
        zone: Option<String>,
    ) -> Self {
        Self {
            start,
            stop,
            repeats,
            zone,
        }
    }

    /// Constructor por defecto (placeholder).
    ///
    /// Valores por defecto:
    /// - `start` = 0
    /// - `stop` = 0
    /// - `repeats` = vacío
    /// - `zone` = None
    pub fn new_blank() -> Self {
        Self {
            start: 0,
            stop: 0,
            repeats: Vec::new(),
            zone: None,
        }
    }

    // --- GETTERS ---
    /// Retorna el tiempo de inicio.
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Retorna el tiempo de fin.
    pub fn stop(&self) -> u64 {
        self.stop
    }

    /// Retorna las líneas de repetición r=.
    pub fn repeats(&self) -> &Vec<String> {
        &self.repeats
    }

    /// Retorna la zona horaria z= (si existe).
    pub fn zone(&self) -> Option<&String> {
        self.zone.as_ref()
    }

    // --- SETTERS ---
    /// Establece el tiempo de inicio.
    pub fn set_start(&mut self, start: u64) {
        self.start = start;
    }

    /// Establece el tiempo de fin.
    pub fn set_stop(&mut self, stop: u64) {
        self.stop = stop;
    }

    /// Establece las líneas de repetición r=.
    pub fn set_repeats(&mut self, repeats: Vec<String>) {
        self.repeats = repeats;
    }

    /// Establece la zona horaria z=.
    pub fn set_zone(&mut self, zone: Option<String>) {
        self.zone = zone;
    }

    /// Agrega una línea de repetición r= al final del vector.
    pub fn add_repeat(&mut self, repeat: String) {
        self.repeats.push(repeat);
    }
}
