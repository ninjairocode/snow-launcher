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
//! # Wine prefix to watch. Absolute, or starting with `~/`.
//! # Defaults to Snow Launcher's own prefix.
//! prefix = "~/Games/battlenet"
//!
//! [sleep]
//! allow_suspend_while_playing = false
//!
//! [display]
//! ui_scale = "auto"   # or a percentage: 100, 125, 150...
//!
//! [house]
//! # Where games live; seen as G: inside Snow Launcher's prefix.
//! library = "~/Games/SnowLauncher"
//! # Proton to use. Defaults to the newest GE-Proton found.
//! # runtime = "~/.local/share/Steam/compatibilitytools.d/GE-Proton11-14-x86_64"
//!
//! [performance]
//! # Your own MangoHud overlay in the games (from the next Battle.net start).
//! mangohud = false
//! # Feral GameMode while a game runs (only the game, never Battle.net).
//! gamemode = false
//! ```

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::display::UiScale;

/// Schema version this build understands.
pub const CURRENT_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema: u32,
    /// Wine prefix to watch, as written in the file (see [`expand_home`]).
    pub prefix: Option<PathBuf>,
    pub sleep: SleepConfig,
    pub display: DisplayConfig,
    pub house: HouseConfig,
    pub performance: PerformanceConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: CURRENT_SCHEMA,
            prefix: None,
            sleep: SleepConfig::default(),
            display: DisplayConfig::default(),
            house: HouseConfig::default(),
            performance: PerformanceConfig::default(),
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// UI scale of the Battle.net app and installer (see `display`).
    pub ui_scale: UiScale,
}

/// Third-party tools the user installed, off by default (`project.md`,
/// section 2). Snow only turns them on the standard way (experiment E15).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    /// `MANGOHUD=1` for Battle.net; the games it opens inherit it (O16).
    pub mangohud: bool,
    /// `snowd` registers each game's process with GameMode while it runs.
    pub gamemode: bool,
}

/// Snow Launcher's own Battle.net environment (the "house").
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct HouseConfig {
    /// Game library, mapped as G: in the prefix. Absolute or `~/...`.
    /// Defaults to `~/Games/SnowLauncher`.
    pub library: Option<PathBuf>,
    /// Proton runtime folder. Defaults to the newest GE-Proton found.
    pub runtime: Option<PathBuf>,
}

/// Replaces a leading `~` component with `home`: `~/Games` becomes
/// `<home>/Games`. Other paths, including `~user/...`, are returned as is.
pub fn expand_home(path: &Path, home: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) if rest.as_os_str().is_empty() => home.to_owned(),
        Ok(rest) => home.join(rest),
        Err(_) => path.to_owned(),
    }
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

    #[test]
    fn reads_prefix() {
        let loaded = parse("prefix = \"~/Games/battlenet\"\n").unwrap();
        assert_eq!(
            loaded.config.prefix,
            Some(PathBuf::from("~/Games/battlenet"))
        );
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn expands_home() {
        let home = Path::new("/home/user");
        let expand = |p: &str| expand_home(Path::new(p), home);
        assert_eq!(
            expand("~/Games/battlenet"),
            Path::new("/home/user/Games/battlenet")
        );
        assert_eq!(expand("~"), Path::new("/home/user"));
        assert_eq!(expand("/opt/prefix"), Path::new("/opt/prefix"));
        // Not supported: kept as is (and later rejected as relative).
        assert_eq!(expand("~other/Games"), Path::new("~other/Games"));
        assert_eq!(expand("Games/battlenet"), Path::new("Games/battlenet"));
    }

    #[test]
    fn reads_display_and_house() {
        let loaded =
            parse("[display]\nui_scale = 150\n[house]\nlibrary = \"~/Games/Snow\"\n").unwrap();
        assert_eq!(loaded.config.display.ui_scale, UiScale::Percent(150));
        assert_eq!(
            loaded.config.house.library,
            Some(PathBuf::from("~/Games/Snow"))
        );
        assert_eq!(loaded.config.house.runtime, None);
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.config.performance, PerformanceConfig::default());

        let loaded = parse("[performance]\nmangohud = true\ngamemode = true\n").unwrap();
        assert!(loaded.config.performance.mangohud && loaded.config.performance.gamemode);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn invalid_ui_scale_is_an_error() {
        assert!(parse("[display]\nui_scale = \"huge\"\n").is_err());
    }
}
