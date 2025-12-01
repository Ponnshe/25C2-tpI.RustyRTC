use std::collections::HashMap;
use std::fs;

/// Represents a configuration file with global settings and named sections.
#[derive(Debug)]
pub struct Config {
    /// Global key-value pairs.
    pub globals: HashMap<String, String>,
    /// Section-specific key-value pairs.
    pub sections: HashMap<String, HashMap<String, String>>,
}

impl Config {
    /// Loads a configuration from a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    pub fn load(path: &str) -> Result<Self, String> {
        let content =
            fs::read_to_string(path).map_err(|e| format!("Error reading file {path}: {e}"))?;

        let mut globals = HashMap::new();
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current_section: Option<String> = None;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                let name = &line[1..line.len() - 1];
                current_section = Some(name.to_string());
                continue;
            }

            if let Some(pos) = line.find('=') {
                let key = line[..pos].trim().to_string();
                let value = line[pos + 1..].trim().trim_matches('"').to_string();

                match &current_section {
                    None => {
                        globals.insert(key, value);
                    }
                    Some(sec) => {
                        sections.entry(sec.clone()).or_default().insert(key, value);
                    }
                }
            }
        }
        Ok(Config { globals, sections })
    }

    /// Creates an empty configuration.
    pub fn empty() -> Self {
        Self {
            globals: HashMap::new(),
            sections: HashMap::new(),
        }
    }

    /// Gets a value from a section.
    #[must_use]
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(section)
            .and_then(|sec| sec.get(key))
            .map(|s| s.as_str())
    }

    /// Gets a non-empty value from a section.
    #[must_use]
    pub fn get_non_empty(&self, section: &str, key: &str) -> Option<&str> {
        self.get(section, key).filter(|s| !s.is_empty())
    }

    /// Gets a global value.
    #[must_use]
    pub fn get_global(&self, key: &str) -> Option<&str> {
        self.globals.get(key).map(|s| s.as_str())
    }

    /// Gets a value from a section or a global value, or a default value.
    #[must_use]
    pub fn get_or_default<'a>(&'a self, section: &str, key: &str, default: &'a str) -> &'a str {
        self.get(section, key)
            .or_else(|| self.get_global(key))
            .unwrap_or(default)
    }

    /// Gets a non-empty value from a section or a global value, or a default value.
    #[must_use]
    pub fn get_non_empty_or_default<'a>(
        &'a self,
        section: &str,
        key: &str,
        default: &'a str,
    ) -> &'a str {
        self.get_non_empty(section, key)
            .or_else(|| self.get_global(key).filter(|s| !s.is_empty()))
            .unwrap_or(default)
    }
}
