//! User configuration (`~/.config/snow-launcher/config.toml`).
//!
//! Parsing is pure: it takes the file's text and returns the configuration
//! plus warnings. Reading the file is up to the caller. Unknown keys and
//! newer schemas produce warnings, never errors (`project.md`, section 9.2).
//!
//! Example:
//!
//! ```toml
//! schema = 1
//!
//! [sleep]
//! allow_suspend_while_playing = false
//! ```

use std::fmt;

use serde::Deserialize;

/// Schema version this build understands.
pub const CURRENT_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema: u32,
    pub sleep: SleepConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: CURRENT_SCHEMA,
            sleep: SleepConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SleepConfig {
    /// Let the computer suspend automatically even while a game is running.
    ///
    /// Off by default: a game left running through a suspend loses its
    /// connection to the server and can take ~15 minutes to notice.
    pub allow_suspend_while_playing: bool,
}

/// Something in the file that was ignored or may not be fully understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigWarning {
    UnknownKey(String),
    NewerSchema { found: u32, supported: u32 },
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey(key) => write!(f, "unknown key `{key}` was ignored"),
            Self::NewerSchema { found, supported } => write!(
                f,
                "config schema {found} is newer than supported ({supported}); \
                 reading what is understood"
            ),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid config file")]
pub struct ConfigError(#[from] toml::de::Error);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub config: Config,
    pub warnings: Vec<ConfigWarning>,
}

/// Parses the text of `config.toml`. Missing keys take their defaults.
///
/// Fails only on invalid TOML or a value of the wrong type.
pub fn parse(text: &str) -> Result<LoadedConfig, ConfigError> {
    let deserializer = toml::Deserializer::parse(text)?;
    let mut warnings = Vec::new();
    let config: Config = serde_ignored::deserialize(deserializer, |path| {
        warnings.push(ConfigWarning::UnknownKey(path.to_string()));
    })?;
    if config.schema > CURRENT_SCHEMA {
        warnings.push(ConfigWarning::NewerSchema {
            found: config.schema,
            supported: CURRENT_SCHEMA,
        });
    }
    Ok(LoadedConfig { config, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        let loaded = parse("").unwrap();
        assert_eq!(loaded.config, Config::default());
        assert!(!loaded.config.sleep.allow_suspend_while_playing);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn reads_sleep_option() {
        let loaded = parse("[sleep]\nallow_suspend_while_playing = true\n").unwrap();
        assert!(loaded.config.sleep.allow_suspend_while_playing);
    }

    #[test]
    fn unknown_keys_are_warnings_not_errors() {
        let loaded = parse("colour = \"blue\"\n[sleep]\nallow_suspend = true\n").unwrap();
        assert_eq!(loaded.config, Config::default());
        assert_eq!(
            loaded.warnings,
            [
                ConfigWarning::UnknownKey("colour".to_owned()),
                ConfigWarning::UnknownKey("sleep.allow_suspend".to_owned()),
            ]
        );
    }

    #[test]
    fn newer_schema_is_a_warning() {
        let loaded = parse("schema = 2\n").unwrap();
        assert_eq!(
            loaded.warnings,
            [ConfigWarning::NewerSchema {
                found: 2,
                supported: CURRENT_SCHEMA
            }]
        );
    }

    #[test]
    fn wrong_type_is_an_error() {
        assert!(parse("[sleep]\nallow_suspend_while_playing = \"yes\"\n").is_err());
        assert!(parse("not toml at all [").is_err());
    }
}
