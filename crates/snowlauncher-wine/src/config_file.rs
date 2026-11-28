//! Reading `config.toml` from disk, changing one setting in it, and
//! choosing which prefix to use.
//!
//! Shared by `snowd` and `snowctl`, so both pick the same prefix.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use snowlauncher_core::config::{self, Config, ConfigError, LoadedConfig, expand_home};

use crate::prefix::default_prefix_path;

/// Why changing a setting failed.
#[derive(Debug, thiserror::Error)]
pub enum UpdateConfigError {
    #[error("could not {what} {}", path.display())]
    Io {
        what: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Edit(#[from] config::ConfigEditError),
}

/// Sets `[section] key = value` (a switch) in `config.toml` at `path`,
/// keeping the rest of the file as it is. The current file, if any, is
/// copied to `backup_dir` first; the new one is written atomically.
/// Returns the backup.
pub fn set_config_bool(
    path: &Path,
    section: &str,
    key: &str,
    value: bool,
    backup_dir: &Path,
) -> Result<Option<PathBuf>, UpdateConfigError> {
    let io = |what, path: &Path| {
        let path = path.to_owned();
        move |source| UpdateConfigError::Io { what, path, source }
    };
    let current = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(io("read", path)(error)),
    };
    let edited = config::set_bool(current.as_deref().unwrap_or(""), section, key, value)?;
    let backup = match &current {
        Some(text) => {
            fs::create_dir_all(backup_dir).map_err(io("create", backup_dir))?;
            let backup = crate::house::unique_backup_path(backup_dir, "config", ".toml");
            crate::house::write_atomic(&backup, text.as_bytes()).map_err(io("write", &backup))?;
            Some(backup)
        }
        None => None,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io("create", dir))?;
    }
    crate::house::write_atomic(path, edited.as_bytes()).map_err(io("write", path))?;
    Ok(backup)
}

/// `$XDG_CONFIG_HOME/snow-launcher/config.toml`.
pub fn default_config_path() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.config_dir().join("snow-launcher").join("config.toml"))
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigFileError {
    #[error("could not read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("invalid config file {path}")]
    Invalid {
        path: PathBuf,
        #[source]
        source: ConfigError,
    },
}

/// Reads and parses the config file. `Ok(None)` if it does not exist,
/// which is normal: every setting has a default.
pub fn read_config(path: &Path) -> Result<Option<LoadedConfig>, ConfigFileError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ConfigFileError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    config::parse(&text)
        .map(Some)
        .map_err(|source| ConfigFileError::Invalid {
            path: path.to_owned(),
            source,
        })
}

/// Where the chosen prefix came from, for logs and messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixSource {
    CommandLine,
    ConfigFile,
    Default,
}

impl fmt::Display for PrefixSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CommandLine => "command line",
            Self::ConfigFile => "config file",
            Self::Default => "default",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PrefixChoiceError {
    #[error(
        "`prefix` in config.toml must be an absolute path or start with `~/` (got {})",
        .0.display()
    )]
    RelativeInConfig(PathBuf),

    #[error("could not determine the home directory")]
    NoHome,
}

/// Picks the prefix: `--prefix` first, then `prefix` in the config file,
/// then Snow Launcher's own prefix.
pub fn choose_prefix(
    command_line: Option<PathBuf>,
    config: &Config,
) -> Result<(PathBuf, PrefixSource), PrefixChoiceError> {
    let home = BaseDirs::new().map(|dirs| dirs.home_dir().to_owned());
    choose_prefix_with(command_line, config, home.as_deref(), default_prefix_path())
}

/// [`choose_prefix`] with the environment passed in, for tests.
fn choose_prefix_with(
    command_line: Option<PathBuf>,
    config: &Config,
    home: Option<&Path>,
    default: Option<PathBuf>,
) -> Result<(PathBuf, PrefixSource), PrefixChoiceError> {
    // A relative `--prefix` is fine: it is relative to where the user
    // typed the command.
    if let Some(path) = command_line {
        return Ok((path, PrefixSource::CommandLine));
    }
    if let Some(configured) = &config.prefix {
        let expanded = match home {
            Some(home) => expand_home(configured, home),
            None if configured.starts_with("~") => return Err(PrefixChoiceError::NoHome),
            None => configured.clone(),
        };
        // A service has no meaningful working directory to resolve
        // a relative path against.
        if !expanded.is_absolute() {
            return Err(PrefixChoiceError::RelativeInConfig(configured.clone()));
        }
        return Ok((expanded, PrefixSource::ConfigFile));
    }
    default
        .map(|path| (path, PrefixSource::Default))
        .ok_or(PrefixChoiceError::NoHome)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/user";
    const DEFAULT: &str = "/home/user/.local/share/snow-launcher/prefixes/battlenet";

    fn choose(
        command_line: Option<&str>,
        configured: Option<&str>,
    ) -> Result<(PathBuf, PrefixSource), PrefixChoiceError> {
        let config = Config {
            prefix: configured.map(PathBuf::from),
            ..Config::default()
        };
        choose_prefix_with(
            command_line.map(PathBuf::from),
            &config,
            Some(Path::new(HOME)),
            Some(PathBuf::from(DEFAULT)),
        )
    }

    #[test]
    fn command_line_wins() {
        let (path, source) = choose(Some("/opt/pfx"), Some("~/Games/battlenet")).unwrap();
        assert_eq!(path, Path::new("/opt/pfx"));
        assert_eq!(source, PrefixSource::CommandLine);
    }

    #[test]
    fn config_prefix_is_expanded() {
        let (path, source) = choose(None, Some("~/Games/battlenet")).unwrap();
        assert_eq!(path, Path::new("/home/user/Games/battlenet"));
        assert_eq!(source, PrefixSource::ConfigFile);
    }

    #[test]
    fn falls_back_to_snow_prefix() {
        let (path, source) = choose(None, None).unwrap();
        assert_eq!(path, Path::new(DEFAULT));
        assert_eq!(source, PrefixSource::Default);
    }

    #[test]
    fn relative_config_prefix_is_rejected() {
        let error = choose(None, Some("Games/battlenet")).unwrap_err();
        assert!(matches!(error, PrefixChoiceError::RelativeInConfig(_)));
        assert_eq!(
            error.to_string(),
            "`prefix` in config.toml must be an absolute path or start with `~/` (got Games/battlenet)"
        );
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            read_config(&tmp.path().join("config.toml"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reads_file_and_reports_invalid_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        fs::write(&path, "prefix = \"~/Games/battlenet\"\n").unwrap();
        let loaded = read_config(&path).unwrap().unwrap();
        assert_eq!(
            loaded.config.prefix,
            Some(PathBuf::from("~/Games/battlenet"))
        );

        fs::write(&path, "prefix = [").unwrap();
        assert!(matches!(
            read_config(&path),
            Err(ConfigFileError::Invalid { .. })
        ));
    }

    #[test]
    fn setting_a_switch_backs_up_and_keeps_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config/snow-launcher/config.toml");
        let backups = tmp.path().join("backups");

        // No file yet: created, nothing to back up.
        assert_eq!(
            set_config_bool(&path, "performance", "gamemode", true, &backups).unwrap(),
            None
        );
        let loaded = read_config(&path).unwrap().unwrap();
        assert!(loaded.config.performance.gamemode);

        let before = fs::read_to_string(&path).unwrap();
        let backup = set_config_bool(&path, "performance", "mangohud", true, &backups)
            .unwrap()
            .unwrap();
        assert_eq!(fs::read_to_string(backup).unwrap(), before);
        let loaded = read_config(&path).unwrap().unwrap();
        assert!(loaded.config.performance.mangohud && loaded.config.performance.gamemode);

        // A broken file is left alone.
        fs::write(&path, "[performance\n").unwrap();
        assert!(set_config_bool(&path, "performance", "mangohud", false, &backups).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[performance\n");
    }
}
