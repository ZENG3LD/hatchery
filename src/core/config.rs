//! Configuration file loader for Hatchery V2.
//!
//! Loads settings from `.hatchery/config.toml` or `hatchery.toml`.
//! Uses a minimal custom TOML parser to avoid adding dependencies.

use std::path::Path;
use anyhow::Result;

/// Parsed configuration from a .hatchery/config.toml file.
#[derive(Debug, Clone, Default)]
pub struct HatcheryFileConfig {
    /// [spawn] section
    pub workers: Option<usize>,
    pub backend: Option<String>,
    pub api_url: Option<String>,
    pub api_model: Option<String>,
    pub max_iterations: Option<usize>,
    pub stall_threshold: Option<usize>,
    pub verbose: Option<bool>,
    pub worktree: Option<bool>,
    pub safe_mode: Option<bool>,
    pub validator: Option<String>,
    pub compaction_threshold: Option<f32>,
    pub event_log: Option<String>,

    /// [queen] section
    pub queen_max_workers: Option<usize>,
    pub queen_model: Option<String>,
    pub queen_timeout_secs: Option<u64>,

}

impl HatcheryFileConfig {
    /// Load config from a file path.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
    }

    /// Try to find and load config from standard locations.
    /// Looks in: .hatchery/config.toml, then hatchery.toml
    pub fn discover(working_dir: &Path) -> Option<Self> {
        let paths = [
            working_dir.join(".hatchery").join("config.toml"),
            working_dir.join("hatchery.toml"),
        ];
        for path in &paths {
            if path.exists() {
                if let Ok(config) = Self::load(path) {
                    return Some(config);
                }
            }
        }
        None
    }

    /// Parse TOML-like content.
    /// Supports: [sections], key = "string", key = 123, key = true, key = 0.8
    pub fn parse(content: &str) -> Result<Self> {
        let mut config = Self::default();
        let mut current_section = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Section header
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                current_section = trimmed[1..trimmed.len()-1].trim().to_string();
                continue;
            }

            // Key-value pair
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let value = trimmed[eq_pos+1..].trim();

                // Remove quotes from string values
                let clean_value = if (value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')) {
                    &value[1..value.len()-1]
                } else {
                    value
                };

                // Map section.key to config fields
                match (current_section.as_str(), key) {
                    ("spawn", "workers") => config.workers = clean_value.parse().ok(),
                    ("spawn", "backend") => config.backend = Some(clean_value.to_string()),
                    ("spawn", "api_url") => config.api_url = Some(clean_value.to_string()),
                    ("spawn", "api_model") => config.api_model = Some(clean_value.to_string()),
                    ("spawn", "max_iterations") => config.max_iterations = clean_value.parse().ok(),
                    ("spawn", "stall_threshold") => config.stall_threshold = clean_value.parse().ok(),
                    ("spawn", "verbose") => config.verbose = clean_value.parse().ok(),
                    ("spawn", "worktree") => config.worktree = clean_value.parse().ok(),
                    ("spawn", "safe_mode") => config.safe_mode = clean_value.parse().ok(),
                    ("spawn", "validator") => config.validator = Some(clean_value.to_string()),
                    ("spawn", "compaction_threshold") => config.compaction_threshold = clean_value.parse().ok(),
                    ("spawn", "event_log") => config.event_log = Some(clean_value.to_string()),
                    ("queen", "max_workers") => config.queen_max_workers = clean_value.parse().ok(),
                    ("queen", "model") => config.queen_model = Some(clean_value.to_string()),
                    ("queen", "timeout_secs") => config.queen_timeout_secs = clean_value.parse().ok(),
                    _ => {} // Ignore unknown keys
                }
            }
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_empty_config() {
        let config = HatcheryFileConfig::parse("").unwrap();
        assert!(config.workers.is_none());
        assert!(config.queen_max_workers.is_none());
    }

    #[test]
    fn test_parse_spawn_section_all_fields() {
        let toml = r#"
[spawn]
workers = 4
backend = "claude"
api_url = "https://api.anthropic.com"
api_model = "claude-sonnet-4-5"
max_iterations = 100
stall_threshold = 3
verbose = true
worktree = true
safe_mode = false
validator = "rust-expert"
compaction_threshold = 0.85
event_log = ".hatchery/events.db"
        "#;

        let config = HatcheryFileConfig::parse(toml).unwrap();
        assert_eq!(config.workers, Some(4));
        assert_eq!(config.backend, Some("claude".to_string()));
        assert_eq!(config.api_url, Some("https://api.anthropic.com".to_string()));
        assert_eq!(config.api_model, Some("claude-sonnet-4-5".to_string()));
        assert_eq!(config.max_iterations, Some(100));
        assert_eq!(config.stall_threshold, Some(3));
        assert_eq!(config.verbose, Some(true));
        assert_eq!(config.worktree, Some(true));
        assert_eq!(config.safe_mode, Some(false));
        assert_eq!(config.validator, Some("rust-expert".to_string()));
        assert_eq!(config.compaction_threshold, Some(0.85));
        assert_eq!(config.event_log, Some(".hatchery/events.db".to_string()));
    }

    #[test]
    fn test_parse_queen_section() {
        let toml = r#"
[queen]
max_workers = 8
model = "claude-opus-4-6"
timeout_secs = 300
        "#;

        let config = HatcheryFileConfig::parse(toml).unwrap();
        assert_eq!(config.queen_max_workers, Some(8));
        assert_eq!(config.queen_model, Some("claude-opus-4-6".to_string()));
        assert_eq!(config.queen_timeout_secs, Some(300));
    }

    #[test]
    fn test_parse_with_comments_and_empty_lines() {
        let toml = r#"
# This is a comment
[spawn]

# Workers configuration
workers = 4

# Another comment

[queen]
max_workers = 8
        "#;

        let config = HatcheryFileConfig::parse(toml).unwrap();
        assert_eq!(config.workers, Some(4));
        assert_eq!(config.queen_max_workers, Some(8));
    }

    #[test]
    fn test_parse_string_values_with_quotes() {
        let toml = r#"
[spawn]
backend = 'claude'
validator = rust-expert
        "#;

        let config = HatcheryFileConfig::parse(toml).unwrap();
        assert_eq!(config.backend, Some("claude".to_string()));
        assert_eq!(config.validator, Some("rust-expert".to_string()));
    }

    #[test]
    fn test_parse_boolean_values() {
        let toml = r#"
[spawn]
verbose = true
worktree = false
safe_mode = true
        "#;

        let config = HatcheryFileConfig::parse(toml).unwrap();
        assert_eq!(config.verbose, Some(true));
        assert_eq!(config.worktree, Some(false));
        assert_eq!(config.safe_mode, Some(true));
    }

    #[test]
    fn test_discover_returns_none_when_no_file_exists() {
        let temp_dir = TempDir::new().unwrap();
        let config = HatcheryFileConfig::discover(temp_dir.path());
        assert!(config.is_none());
    }

    #[test]
    fn test_discover_finds_dotdir_config() {
        let temp_dir = TempDir::new().unwrap();
        let hatchery_dir = temp_dir.path().join(".hatchery");
        fs::create_dir(&hatchery_dir).unwrap();

        let config_path = hatchery_dir.join("config.toml");
        fs::write(&config_path, "[spawn]\nworkers = 4\n").unwrap();

        let config = HatcheryFileConfig::discover(temp_dir.path()).unwrap();
        assert_eq!(config.workers, Some(4));
    }

    #[test]
    fn test_discover_finds_root_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("hatchery.toml");
        fs::write(&config_path, "[queen]\nmax_workers = 6\n").unwrap();

        let config = HatcheryFileConfig::discover(temp_dir.path()).unwrap();
        assert_eq!(config.queen_max_workers, Some(6));
    }

    #[test]
    fn test_discover_prefers_dotdir_over_root() {
        let temp_dir = TempDir::new().unwrap();

        // Create .hatchery/config.toml
        let hatchery_dir = temp_dir.path().join(".hatchery");
        fs::create_dir(&hatchery_dir).unwrap();
        let dotdir_config = hatchery_dir.join("config.toml");
        fs::write(&dotdir_config, "[spawn]\nworkers = 4\n").unwrap();

        // Create hatchery.toml
        let root_config = temp_dir.path().join("hatchery.toml");
        fs::write(&root_config, "[spawn]\nworkers = 8\n").unwrap();

        let config = HatcheryFileConfig::discover(temp_dir.path()).unwrap();
        // Should prefer .hatchery/config.toml
        assert_eq!(config.workers, Some(4));
    }

    #[test]
    fn test_parse_float_values() {
        let toml = r#"
[spawn]
compaction_threshold = 0.75
        "#;

        let config = HatcheryFileConfig::parse(toml).unwrap();
        assert_eq!(config.compaction_threshold, Some(0.75));
    }
}
