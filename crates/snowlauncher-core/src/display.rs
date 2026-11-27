//! UI scale of the Battle.net app and its installer (Wine DPI).
//!
//! Wine draws everything at 96 DPI and does not follow the desktop's
//! scale, so on a scaled screen the Battle.net app looks tiny. Rather than
//! guessing, Snow Launcher asks the desktop: GNOME and KDE publish, for X
//! apps (where Wine draws), the DPI that fits the chosen scale (`Xft.dpi`).
//! Validated in experiment E8 (`docs/experimentos.md`, O7).

use std::fmt;

use serde::Deserialize;

/// Wine's (and Windows') base DPI: 100%.
pub const BASE_DPI: u32 = 96;
/// Limits: 100% to 300%.
pub const MIN_DPI: u32 = 96;
pub const MAX_DPI: u32 = 288;
/// Windows-style 25% steps; values in between blur text.
pub const DPI_STEP: u32 = 24;

/// `ui_scale` in `config.toml`: `"auto"` or a percentage (`150`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "UiScaleRaw")]
pub enum UiScale {
    #[default]
    Auto,
    Percent(u32),
}

/// What may be written in the file, before validation.
#[derive(Deserialize)]
#[serde(untagged)]
enum UiScaleRaw {
    Text(String),
    Number(i64),
}

impl TryFrom<UiScaleRaw> for UiScale {
    type Error = String;

    fn try_from(raw: UiScaleRaw) -> Result<Self, String> {
        match raw {
            UiScaleRaw::Text(text) if text == "auto" => Ok(Self::Auto),
            UiScaleRaw::Text(text) => Err(format!(
                "ui_scale must be \"auto\" or a percentage, not \"{text}\""
            )),
            UiScaleRaw::Number(percent) => u32::try_from(percent)
                .ok()
                .filter(|percent| (50..=400).contains(percent))
                .map(Self::Percent)
                .ok_or_else(|| format!("ui_scale must be between 50 and 400, not {percent}")),
        }
    }
}

/// Where the chosen DPI came from, for logs and `doctor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpiSource {
    /// A percentage set by the user.
    UserSetting,
    /// The desktop's `Xft.dpi`.
    Desktop,
    /// Nothing known: Wine's default.
    Default,
}

impl fmt::Display for DpiSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UserSetting => "your ui_scale setting",
            Self::Desktop => "the desktop",
            Self::Default => "Wine's default",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpiChoice {
    pub dpi: u32,
    pub source: DpiSource,
}

impl DpiChoice {
    /// The DPI as a percentage of the base (192 → 200).
    pub fn percent(self) -> u32 {
        self.dpi * 100 / BASE_DPI
    }
}

/// Picks the Wine DPI: the user's percentage, else the desktop's
/// `Xft.dpi`, else 96. Clamped to 96–288 and rounded to 25% steps.
pub fn choose_dpi(setting: UiScale, desktop_dpi: Option<u32>) -> DpiChoice {
    let (raw, source) = match (setting, desktop_dpi) {
        (UiScale::Percent(percent), _) => (BASE_DPI * percent / 100, DpiSource::UserSetting),
        (UiScale::Auto, Some(dpi)) if dpi > 0 => (dpi, DpiSource::Desktop),
        (UiScale::Auto, _) => (BASE_DPI, DpiSource::Default),
    };
    let rounded = (raw + DPI_STEP / 2) / DPI_STEP * DPI_STEP;
    DpiChoice {
        dpi: rounded.clamp(MIN_DPI, MAX_DPI),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(desktop: Option<u32>) -> u32 {
        choose_dpi(UiScale::Auto, desktop).dpi
    }

    /// The desktop setups listed in project.md, section 5.
    #[test]
    fn follows_the_desktop() {
        assert_eq!(auto(Some(192)), 192); // GNOME Wayland 125%/150%/200% (validated at 125%)
        assert_eq!(auto(Some(96)), 96); // GNOME 100%, or KDE scaling X apps itself
        assert_eq!(auto(Some(120)), 120); // KDE 125%, X apps scale themselves
        assert_eq!(auto(Some(144)), 144); // KDE 150%
        assert_eq!(auto(None), 96); // nothing published
        assert_eq!(choose_dpi(UiScale::Auto, None).source, DpiSource::Default);
        assert_eq!(
            choose_dpi(UiScale::Auto, Some(192)).source,
            DpiSource::Desktop
        );
    }

    #[test]
    fn user_percentage_wins() {
        let choice = choose_dpi(UiScale::Percent(150), Some(192));
        assert_eq!(choice.dpi, 144);
        assert_eq!(choice.source, DpiSource::UserSetting);
        assert_eq!(choice.percent(), 150);
    }

    #[test]
    fn rounds_to_steps_and_clamps() {
        assert_eq!(auto(Some(110)), 120); // between steps: nearest
        assert_eq!(auto(Some(100)), 96);
        assert_eq!(auto(Some(400)), 288); // more than 300%
        assert_eq!(auto(Some(0)), 96); // nonsense from the desktop
        assert_eq!(choose_dpi(UiScale::Percent(50), None).dpi, 96);
    }

    #[test]
    fn parses_from_config_text() {
        #[derive(Deserialize)]
        struct Wrapper {
            ui_scale: UiScale,
        }
        let parse = |text: &str| toml::from_str::<Wrapper>(text).map(|w| w.ui_scale);
        assert_eq!(parse("ui_scale = \"auto\"").unwrap(), UiScale::Auto);
        assert_eq!(parse("ui_scale = 150").unwrap(), UiScale::Percent(150));
        assert!(parse("ui_scale = \"big\"").is_err());
        assert!(parse("ui_scale = 1000").is_err());
        assert!(parse("ui_scale = -5").is_err());
    }
}
