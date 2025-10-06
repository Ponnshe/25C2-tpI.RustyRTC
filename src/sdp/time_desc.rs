/// Represents a time block (`t=`) in SDP (Session Description Protocol),
/// including repetitions (`r=`) and time zones (`z=`).
#[derive(Debug)]
pub struct TimeDesc {
    start: u64,           // start time in NTP seconds, usually 0
    stop: u64,            // end time in NTP seconds, usually 0
    repeats: Vec<String>, // raw r= lines
    zone: Option<String>, // raw z= line (time zone)
}

impl TimeDesc {
    /// Full constructor.
    ///
    /// # Parameters
    /// - `start`: start time in NTP seconds
    /// - `stop`: end time in NTP seconds
    /// - `repeats`: vector of raw r= lines
    /// - `zone`: optional raw z= line
    pub const fn new(start: u64, stop: u64, repeats: Vec<String>, zone: Option<String>) -> Self {
        Self {
            start,
            stop,
            repeats,
            zone,
        }
    }

    /// Default constructor (placeholder).
    ///
    /// Default values:
    /// - `start` = 0
    /// - `stop` = 0
    /// - `repeats` = empty
    /// - `zone` = None
    pub const fn new_blank() -> Self {
        Self {
            start: 0,
            stop: 0,
            repeats: Vec::new(),
            zone: None,
        }
    }

    // --- GETTERS ---
    /// Returns the start time.
    pub const fn start(&self) -> u64 {
        self.start
    }

    /// Returns the end time.
    pub const fn stop(&self) -> u64 {
        self.stop
    }

    /// Returns the repetition r= lines.
    pub const fn repeats(&self) -> &Vec<String> {
        &self.repeats
    }

    /// Returns the z= time zone (if present).
    pub const fn zone(&self) -> Option<&String> {
        self.zone.as_ref()
    }

    // --- SETTERS ---
    /// Sets the start time.
    pub const fn set_start(&mut self, start: u64) {
        self.start = start;
    }

    /// Sets the end time.
    pub const fn set_stop(&mut self, stop: u64) {
        self.stop = stop;
    }

    /// Sets the repetition r= lines.
    pub fn set_repeats(&mut self, repeats: Vec<String>) {
        self.repeats = repeats;
    }

    /// Sets the z= time zone.
    pub fn set_zone(&mut self, zone: Option<String>) {
        self.zone = zone;
    }

    /// Appends a repetition r= line to the end of the vector.
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
