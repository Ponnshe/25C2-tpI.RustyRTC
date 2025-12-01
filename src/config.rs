use std::collections::HashMap;
use std::fs;

#[derive(Debug)]
pub struct Config {
    pub globals: HashMap<String, String>,
    pub sections: HashMap<String, HashMap<String, String>>,
}

impl Config {
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

    pub fn empty() -> Self {
        Self {
            globals: HashMap::new(),
            sections: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(section)
            .and_then(|sec| sec.get(key))
            .map(|s| s.as_str())
    }

    #[must_use]
    pub fn get_non_empty(&self, section: &str, key: &str) -> Option<&str> {
        self.get(section, key).filter(|s| !s.is_empty())
    }

    #[must_use]
    pub fn get_global(&self, key: &str) -> Option<&str> {
        self.globals.get(key).map(|s| s.as_str())
    }

    #[must_use]
    pub fn get_or_default<'a>(&'a self, section: &str, key: &str, default: &'a str) -> &'a str {
        self.get(section, key)
            .or_else(|| self.get_global(key))
            .unwrap_or(default)
    }

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
