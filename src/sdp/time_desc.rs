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
    pub fn new(start: u64, stop: u64, repeats: Vec<String>, zone: Option<String>) -> Self {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::TimeDesc;

    #[test]
    fn new_sets_fields_correctly() {
        let repeats = vec![
            "r=7d 1h 0s 0 3600".to_string(),
            "r=604800 3600 0 3600".to_string(),
        ];
        let zone = Some("z=2882844526 -1h 2898848070 0".to_string());

        let t = TimeDesc::new(0, 0, repeats.clone(), zone.clone());

        assert_eq!(t.start(), 0);
        assert_eq!(t.stop(), 0);
        assert_eq!(t.repeats(), &repeats);
        assert_eq!(t.zone(), zone.as_ref());
    }

    #[test]
    fn new_blank_defaults() {
        let t = TimeDesc::new_blank();

        assert_eq!(t.start(), 0);
        assert_eq!(t.stop(), 0);
        assert!(t.repeats().is_empty());
        assert!(t.zone().is_none());
    }

    #[test]
    fn setters_update_fields_and_add_repeat_appends() {
        let mut t = TimeDesc::new_blank();

        t.set_start(100);
        t.set_stop(200);
        t.set_repeats(vec!["r=60 10 0 10".into()]);
        t.set_zone(Some("z=0 0".into()));
        t.add_repeat("r=120 20 0 20".into());

        assert_eq!(t.start(), 100);
        assert_eq!(t.stop(), 200);
        assert_eq!(t.repeats().len(), 2);
        assert_eq!(t.repeats()[0], "r=60 10 0 10");
        assert_eq!(t.repeats()[1], "r=120 20 0 20");
        assert_eq!(t.zone(), Some(&"z=0 0".to_string()));

        // Clear zone
        t.set_zone(None);
        assert!(t.zone().is_none());
    }

    #[test]
    fn accepts_empty_strings_in_repeats_and_zone() {
        let mut t = TimeDesc::new(1, 2, vec![], Some(String::new()));
        assert_eq!(t.zone(), Some(&String::new()));

        t.add_repeat(String::new());
        assert_eq!(t.repeats().len(), 1);
        assert_eq!(t.repeats()[0], "");
    }

    #[test]
    fn handles_extreme_ntp_values() {
        let max = u64::MAX;
        let min = 0u64;

        // start/stop at extremes
        let t = TimeDesc::new(max, min, vec![], None);

        // The struct stores raw values; validation (e.g., start <= stop) is external.
        assert_eq!(t.start(), max);
        assert_eq!(t.stop(), min);
        assert!(t.repeats().is_empty());
        assert!(t.zone().is_none());
    }

    #[test]
    fn allows_stop_before_start_without_panicking() {
        // Border case often seen during parsing of partially-specified SDP.
        let mut t = TimeDesc::new_blank();
        t.set_start(1_000);
        t.set_stop(999);

        assert_eq!(t.start(), 1_000);
        assert_eq!(t.stop(), 999);
        // No invariants enforced here; higher-level validation should flag this if needed.
    }

    #[test]
    fn large_number_of_repeats_is_supported() {
        let mut t = TimeDesc::new_blank();
        for i in 0..10_000 {
            t.add_repeat("r=86400 ".to_string() + &i.to_string() + " 0 3600");
        }
        assert_eq!(t.repeats().len(), 10_000);
        // spot check start/end
        assert!(t.repeats().first().unwrap().starts_with("r=86400 "));
        assert!(t.repeats().last().unwrap().starts_with("r=86400 "));
    }
}
